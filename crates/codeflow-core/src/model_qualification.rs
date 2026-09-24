//! Human-approved model+harness qualification records and project selection.
//!
//! Qualification records are evidence indexes, not runtime routing
//! configuration. A project may reference those records by stable role, but
//! neither surface launches a harness or accepts executable configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const MAX_RECORD_BYTES: u64 = 1024 * 1024;
const HARNESS_CATALOG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-evaluate-model/resources/harnesses.json"
));
const CURRENT_ENSEMBLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json"
));
const ROUTING_POLICY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-model-orchestrator/resources/routing-policy.json"
));
const PROJECT_SELECTION_PATH: &str = ".codeflow/model-selection.json";

/// Trusted, source-controlled metadata for a capability-supported native harness.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessMetadata {
    pub id: String,
    pub provider: String,
    pub lineage: String,
    pub status: String,
    pub capabilities: Vec<String>,
    pub version_probe: Option<String>,
    pub evidence: BTreeMap<String, Vec<String>>,
}

/// One code-allowlisted harness version probe.
#[derive(Debug, Clone, Copy)]
pub struct TrustedVersionProbe {
    pub command: &'static str,
    pub args: &'static [&'static str],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessCatalog {
    schema_version: u64,
    capability_contract: Vec<String>,
    harnesses: Vec<HarnessMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnsembleCatalog {
    schema_version: u64,
    policy_id: String,
    standing_roles: Vec<String>,
    design_execution_owner: String,
    bindings: Vec<EnsembleBinding>,
    high_triggers: Vec<String>,
    xhigh_triggers: Vec<String>,
    rules: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingPolicy {
    schema_version: u64,
    policy_id: String,
    default_review: String,
    design_production: String,
    host_does_not_own_duty: bool,
    extra_family_review: ExtraFamilyReview,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtraFamilyReview {
    never_silent_vote: bool,
    requires_named_assignment: bool,
    invoke_when_available: bool,
    triggers: Vec<String>,
    rule: String,
}

/// One stable primary role and its fully managed shipped default.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsembleBinding {
    pub role: String,
    pub seat: String,
    pub provider: String,
    pub lineage: String,
    pub model_class: String,
    pub native_selectors: BTreeMap<String, String>,
    pub default_effort: String,
    pub escalation_effort: String,
    pub responsibilities: Vec<String>,
    pub internal_routes: Vec<InternalRoute>,
}

/// A harness-owned worker class that never replaces its primary.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalRoute {
    pub route_id: String,
    pub model_class: String,
    pub native_selectors: BTreeMap<String, String>,
    pub efforts: Vec<RouteEffort>,
    pub default_effort: RouteEffort,
    pub workloads: Vec<RouteWorkload>,
    pub status: RouteStatus,
    pub evidence: Vec<String>,
    #[serde(rename = "use")]
    pub use_case: String,
}

/// One exact effort accepted by a managed worker route.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum RouteEffort {
    Low,
    Medium,
    High,
    Xhigh,
}

impl RouteEffort {
    /// Stable JSON spelling used in diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

/// A bounded task class declared by a managed worker route.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RouteWorkload {
    Reasoning,
    Implementation,
    Evidence,
    ReviewSupport,
    DesignImplementation,
}

impl RouteWorkload {
    /// Stable JSON spelling used in diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Implementation => "implementation",
            Self::Evidence => "evidence",
            Self::ReviewSupport => "review-support",
            Self::DesignImplementation => "design-implementation",
        }
    }
}

/// Evidence status of a managed worker route, never a primary promotion.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteStatus {
    Candidate,
    ScopedQualified,
}

impl RouteStatus {
    /// Stable JSON spelling used in diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::ScopedQualified => "scoped-qualified",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSelection {
    schema_version: u64,
    bindings: Vec<ProjectSelectionEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSelectionEntry {
    role: String,
    binding_id: String,
}

/// One qualified project override after role, lineage, and harness validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSelection {
    pub role: String,
    pub seat: String,
    pub binding_id: String,
    pub provider: String,
    pub lineage: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
}

/// One promoted model+harness binding.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedBinding {
    pub schema_version: u64,
    pub binding_id: String,
    pub provider: String,
    pub lineage: String,
    pub eligible_roles: Vec<String>,
    pub qualified_at: String,
    pub requested: RequestedBinding,
    pub observed: ObservedBinding,
    pub qualification: QualificationEvidence,
    #[serde(default)]
    pub settings_sources: Vec<SettingsSource>,
    pub approval: Approval,
}

/// Requested binding recorded by the evaluated native session.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedBinding {
    pub model: String,
    pub effort: String,
    pub harness: String,
    pub harness_version: String,
    pub settings_digest: String,
}

/// Native observation proving the requested model and effort.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedBinding {
    pub model: String,
    pub effort: String,
    pub evidence: Vec<ObservedEvidence>,
}

/// Content-addressed observation retained without potentially sensitive refs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidence {
    pub kind: String,
    pub digest: String,
}

/// Full-suite result that justified promotion.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationEvidence {
    pub run_id: String,
    pub suite: String,
    pub suite_digest: String,
    pub result_digest: String,
    pub codeflow_revision: String,
}

/// Optional live settings input whose digest doctor can recompute.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSource {
    pub path: PathBuf,
    pub digest: String,
}

/// Explicit human approval retained from the full evaluation result.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub reviewer: String,
    pub reviewed_at: String,
}

/// Parse and validate the source-controlled native harness catalog.
///
/// # Errors
///
/// Returns an error when the embedded catalog is malformed or fails its
/// capability contract.
pub fn harness_catalog() -> Result<BTreeMap<String, HarnessMetadata>, String> {
    let catalog: HarnessCatalog =
        serde_json::from_str(HARNESS_CATALOG).map_err(|error| error.to_string())?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "unsupported harness catalog schema {}",
            catalog.schema_version
        ));
    }
    let required: BTreeSet<&str> = catalog
        .capability_contract
        .iter()
        .map(String::as_str)
        .collect();
    if required.is_empty() {
        return Err("harness capability contract is empty".into());
    }
    if required.len() != catalog.capability_contract.len() {
        return Err("harness capability contract contains a duplicate".into());
    }
    let mut indexed = BTreeMap::new();
    for harness in catalog.harnesses {
        validate_nonempty(&harness.id, "harness.id")?;
        validate_nonempty(&harness.provider, "harness.provider")?;
        validate_nonempty(&harness.lineage, "harness.lineage")?;
        if harness.status != "capability-supported" {
            return Err(format!(
                "harness {} is not capability-supported",
                harness.id
            ));
        }
        let actual: BTreeSet<&str> = harness.capabilities.iter().map(String::as_str).collect();
        if actual.len() != harness.capabilities.len() {
            return Err(format!(
                "harness {} contains a duplicate capability",
                harness.id
            ));
        }
        let missing: Vec<&str> = required.difference(&actual).copied().collect();
        if !missing.is_empty() {
            return Err(format!(
                "harness {} misses capabilities: {}",
                harness.id,
                missing.join(", ")
            ));
        }
        if harness
            .evidence
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != actual
        {
            return Err(format!(
                "harness {} evidence does not map every declared capability",
                harness.id
            ));
        }
        if harness
            .evidence
            .values()
            .any(|references| references.is_empty() || references.iter().any(String::is_empty))
        {
            return Err(format!("harness {} has incomplete evidence", harness.id));
        }
        if let Some(probe_id) = &harness.version_probe {
            validate_safe_atom(probe_id, "version probe id")?;
            if trusted_version_probe(probe_id).is_none() {
                return Err(format!(
                    "harness {} names unknown version probe {probe_id}",
                    harness.id
                ));
            }
        }
        let id = harness.id.clone();
        if indexed.insert(id.clone(), harness).is_some() {
            return Err(format!("duplicate harness {id}"));
        }
    }
    Ok(indexed)
}

fn validate_ensemble_triggers(ensemble: &EnsembleCatalog) -> Result<(), String> {
    if ensemble.high_triggers.is_empty()
        || ensemble.xhigh_triggers.is_empty()
        || ensemble.rules.is_empty()
    {
        return Err("current ensemble effort triggers and rules must not be empty".into());
    }
    Ok(())
}

/// Parse and validate the fully managed current ensemble.
///
/// # Errors
///
/// Returns an error when roles, seats, lineages, selectors, or required
/// responsibility fields are missing or contradictory.
pub fn current_ensemble() -> Result<BTreeMap<String, EnsembleBinding>, String> {
    parse_legacy_ensemble(CURRENT_ENSEMBLE.as_bytes())
}

/// Transitional schema 4 reader, removed with the managed schema 5 switch.
pub(crate) fn parse_legacy_ensemble(
    bytes: &[u8],
) -> Result<BTreeMap<String, EnsembleBinding>, String> {
    let ensemble: EnsembleCatalog =
        crate::strict_json::parse_strict_json(bytes).map_err(|error| error.to_string())?;
    validate_parsed_ensemble(ensemble)
}

fn validate_parsed_ensemble(
    ensemble: EnsembleCatalog,
) -> Result<BTreeMap<String, EnsembleBinding>, String> {
    if ensemble.schema_version != 4 {
        return Err(format!(
            "unsupported current ensemble schema {}",
            ensemble.schema_version
        ));
    }
    validate_nonempty(&ensemble.policy_id, "ensemble.policy_id")?;
    validate_routing_policy(&ensemble.policy_id)?;
    if ensemble.standing_roles.len() != 2 {
        return Err("the standing pair must define exactly two primary roles".into());
    }
    if ensemble.bindings.len() < 2 {
        return Err("current ensemble must include the standing pair".into());
    }
    let mut standing = BTreeSet::new();
    for role in &ensemble.standing_roles {
        validate_safe_atom(role, "ensemble standing role")?;
        if !standing.insert(role.clone()) {
            return Err(format!("duplicate standing role {role}"));
        }
    }
    validate_safe_atom(
        &ensemble.design_execution_owner,
        "ensemble design_execution_owner",
    )?;
    if !standing.contains(&ensemble.design_execution_owner) {
        return Err("ensemble design_execution_owner must be a standing role".into());
    }
    if ensemble.policy_id == "claude-codex-duo"
        && standing
            != BTreeSet::from([
                "claude-judgment-primary".to_string(),
                "codex-engineering-primary".to_string(),
            ])
    {
        return Err(
            "claude-codex-duo standing pair must remain claude-judgment-primary and codex-engineering-primary"
                .into(),
        );
    }
    validate_ensemble_triggers(&ensemble)?;
    let catalog = harness_catalog()?;
    let mut roles = BTreeMap::new();
    let mut seats = BTreeSet::new();
    let mut lineages = BTreeSet::new();
    for binding in ensemble.bindings {
        validate_safe_atom(&binding.role, "ensemble role")?;
        validate_safe_atom(&binding.seat, "ensemble seat")?;
        for (value, label) in [
            (&binding.provider, "ensemble provider"),
            (&binding.lineage, "ensemble lineage"),
            (&binding.model_class, "ensemble model_class"),
            (&binding.default_effort, "ensemble default_effort"),
            (&binding.escalation_effort, "ensemble escalation_effort"),
        ] {
            validate_nonempty(value, label)?;
        }
        if !seats.insert(binding.seat.clone()) {
            return Err(format!("duplicate ensemble seat {}", binding.seat));
        }
        if !lineages.insert(binding.lineage.clone()) {
            return Err(format!(
                "primary lineage {} appears more than once",
                binding.lineage
            ));
        }
        if binding.native_selectors.is_empty() || binding.responsibilities.is_empty() {
            return Err(format!(
                "ensemble role {} has no selector or responsibility",
                binding.role
            ));
        }
        for (harness_id, selector) in &binding.native_selectors {
            validate_nonempty(selector, "ensemble native selector")?;
            let harness = catalog.get(harness_id).ok_or_else(|| {
                format!(
                    "ensemble role {} uses unsupported harness {harness_id}",
                    binding.role
                )
            })?;
            if harness.provider != binding.provider || harness.lineage != binding.lineage {
                return Err(format!(
                    "ensemble role {} mismatches harness {harness_id}",
                    binding.role
                ));
            }
        }
        validate_internal_routes(&binding, &ensemble.design_execution_owner)?;
        let role = binding.role.clone();
        if roles.insert(role.clone(), binding).is_some() {
            return Err(format!("duplicate ensemble role {role}"));
        }
    }
    for role in &standing {
        if !roles.contains_key(role) {
            return Err(format!("standing role {role} is missing from bindings"));
        }
    }
    validate_design_execution_owner_lineage(&roles, &ensemble.design_execution_owner)?;
    Ok(roles)
}

fn validate_design_execution_owner_lineage(
    roles: &BTreeMap<String, EnsembleBinding>,
    design_execution_owner: &str,
) -> Result<(), String> {
    if roles[design_execution_owner].lineage == "claude" {
        Ok(())
    } else {
        Err("ensemble design_execution_owner must have Claude lineage".into())
    }
}

fn validate_internal_routes(
    binding: &EnsembleBinding,
    design_execution_owner: &str,
) -> Result<(), String> {
    let mut route_ids = BTreeSet::new();
    for route in &binding.internal_routes {
        validate_safe_atom(&route.route_id, "internal route route_id")?;
        if !route_ids.insert(route.route_id.as_str()) {
            return Err(format!(
                "ensemble role {} contains duplicate internal route {}",
                binding.role, route.route_id
            ));
        }
        validate_nonempty(&route.model_class, "internal route model_class")?;
        if route.native_selectors.is_empty() {
            return Err(format!(
                "internal route {} has no native selector",
                route.route_id
            ));
        }
        for (harness_id, selector) in &route.native_selectors {
            validate_nonempty(selector, "internal route native selector")?;
            if !binding.native_selectors.contains_key(harness_id) {
                return Err(format!(
                    "internal route {} uses harness {harness_id} outside parent role {}",
                    route.route_id, binding.role
                ));
            }
        }
        if route.efforts.is_empty() {
            return Err(format!("internal route {} has no effort", route.route_id));
        }
        let efforts: BTreeSet<RouteEffort> = route.efforts.iter().copied().collect();
        if efforts.len() != route.efforts.len() {
            return Err(format!(
                "internal route {} contains a duplicate effort",
                route.route_id
            ));
        }
        if !efforts.contains(&route.default_effort) {
            return Err(format!(
                "internal route {} default effort is not in efforts",
                route.route_id
            ));
        }
        if route.workloads.is_empty() {
            return Err(format!("internal route {} has no workload", route.route_id));
        }
        let workloads: BTreeSet<RouteWorkload> = route.workloads.iter().copied().collect();
        if workloads.len() != route.workloads.len() {
            return Err(format!(
                "internal route {} contains a duplicate workload",
                route.route_id
            ));
        }
        if workloads.contains(&RouteWorkload::DesignImplementation)
            && binding.role != design_execution_owner
        {
            return Err(format!(
                "internal route {} declares design-implementation outside design_execution_owner {design_execution_owner}",
                route.route_id
            ));
        }
        if route.status == RouteStatus::ScopedQualified && route.evidence.is_empty() {
            return Err(format!(
                "scoped-qualified internal route {} has no evidence",
                route.route_id
            ));
        }
        let mut evidence = BTreeSet::new();
        for path in &route.evidence {
            validate_repository_relative_reference(path, "internal route evidence")?;
            if !evidence.insert(path.as_str()) {
                return Err(format!(
                    "internal route {} contains duplicate evidence",
                    route.route_id
                ));
            }
        }
        validate_nonempty(&route.use_case, "internal route use")?;
    }
    Ok(())
}

fn validate_repository_relative_reference(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > 1024
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with([' ', '.'])
                || part
                    .chars()
                    .any(|character| character.is_control() || "<>\"|?*".contains(character))
        })
    {
        return Err(format!(
            "{label} must be a portable repository-relative path without traversal"
        ));
    }
    Ok(())
}

fn routing_policy() -> Result<&'static RoutingPolicy, String> {
    static POLICY: OnceLock<Result<RoutingPolicy, String>> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            let policy: RoutingPolicy =
                serde_json::from_str(ROUTING_POLICY).map_err(|error| error.to_string())?;
            validate_parsed_routing_policy(&policy, &policy.policy_id)?;
            Ok(policy)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Shared immutable policy facts; parsing and validation happen once.
pub(crate) fn routing_policy_triggers() -> Result<&'static [String], String> {
    Ok(&routing_policy()?.extra_family_review.triggers)
}

fn validate_routing_policy(policy_id: &str) -> Result<(), String> {
    validate_parsed_routing_policy(routing_policy()?, policy_id)
}

fn validate_parsed_routing_policy(policy: &RoutingPolicy, policy_id: &str) -> Result<(), String> {
    if policy.schema_version != 1 {
        return Err(format!(
            "unsupported routing policy schema {}",
            policy.schema_version
        ));
    }
    validate_nonempty(&policy.policy_id, "routing policy_id")?;
    if policy.policy_id != policy_id {
        return Err("routing policy_id must match the current ensemble".into());
    }
    if policy.default_review != "standing-pair" {
        return Err("routing policy default_review must be standing-pair".into());
    }
    if policy.design_production != "claude-native-session" {
        return Err("routing policy must keep Claude native-session design production".into());
    }
    if !policy.host_does_not_own_duty {
        return Err("routing policy must keep host-does-not-own-duty".into());
    }
    if !policy.extra_family_review.never_silent_vote
        || !policy.extra_family_review.requires_named_assignment
        || !policy.extra_family_review.invoke_when_available
        || policy.extra_family_review.triggers.is_empty()
        || policy.extra_family_review.rule.is_empty()
    {
        return Err("extra-family review must be named, triggered, invoked when available, and never a silent vote".into());
    }
    Ok(())
}

/// Resolve a project's reference-only overrides against approved bindings.
///
/// An absent or empty project selection returns no overrides. Any invalid entry
/// rejects the entire selection so callers cannot partially apply it.
///
/// # Errors
///
/// Returns an error for malformed or unsafe project configuration, unknown or
/// ineligible binding records, role/harness/provider mismatches, duplicates,
/// or collapsed primary lineages.
pub fn resolve_project_selection(
    project_root: &Path,
    records: &[QualifiedBinding],
) -> Result<Vec<ResolvedSelection>, String> {
    let path = project_root.join(PROJECT_SELECTION_PATH);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("stat {}: {error}", path.display())),
    }
    let data = read_bounded_json(&path, "model selection")?;
    let selection: ProjectSelection = serde_json::from_slice(&data)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    if selection.schema_version != 1 {
        return Err(format!(
            "unsupported project model-selection schema {}",
            selection.schema_version
        ));
    }
    if selection.bindings.is_empty() {
        return Ok(Vec::new());
    }
    let ensemble = current_ensemble()?;
    let records_by_id: BTreeMap<&str, &QualifiedBinding> = records
        .iter()
        .map(|record| (record.binding_id.as_str(), record))
        .collect();
    let mut resolved = Vec::with_capacity(selection.bindings.len());
    let mut role_harnesses = BTreeSet::new();
    for entry in selection.bindings {
        validate_safe_atom(&entry.role, "selection role")?;
        validate_safe_atom(&entry.binding_id, "selection binding_id")?;
        let role = ensemble
            .get(&entry.role)
            .ok_or_else(|| format!("unknown project model-selection role {}", entry.role))?;
        let record = records_by_id
            .get(entry.binding_id.as_str())
            .ok_or_else(|| {
                format!(
                    "project model selection references missing binding {}",
                    entry.binding_id
                )
            })?;
        if !record.eligible_roles.contains(&entry.role) {
            return Err(format!(
                "binding {} is not qualified for role {}",
                entry.binding_id, entry.role
            ));
        }
        if record.provider != role.provider || record.lineage != role.lineage {
            return Err(format!(
                "binding {} provider/lineage does not match role {}",
                entry.binding_id, entry.role
            ));
        }
        if !role
            .native_selectors
            .contains_key(&record.requested.harness)
        {
            return Err(format!(
                "binding {} harness {} is not supported by role {}",
                entry.binding_id, record.requested.harness, entry.role
            ));
        }
        if !role_harnesses.insert((entry.role.clone(), record.requested.harness.clone())) {
            return Err(format!(
                "project model selection repeats role {} for harness {}",
                entry.role, record.requested.harness
            ));
        }
        resolved.push(ResolvedSelection {
            role: entry.role,
            seat: role.seat.clone(),
            binding_id: entry.binding_id,
            provider: record.provider.clone(),
            lineage: record.lineage.clone(),
            harness: record.requested.harness.clone(),
            model: record.requested.model.clone(),
            effort: record.requested.effort.clone(),
        });
    }

    let selected_by_role: BTreeMap<&str, &ResolvedSelection> = resolved
        .iter()
        .map(|selection| (selection.role.as_str(), selection))
        .collect();
    // Provider/lineage checks above make collapse impossible for the current
    // roles. Keep this backstop so a future schema change cannot weaken the
    // independent-lineage invariant unnoticed.
    let effective_lineages: BTreeSet<&str> = ensemble
        .values()
        .map(|role| {
            selected_by_role
                .get(role.role.as_str())
                .map_or(role.lineage.as_str(), |selected| selected.lineage.as_str())
        })
        .collect();
    if effective_lineages.len() != ensemble.len() {
        return Err("project model selection collapses independent primary lineages".into());
    }
    resolved.sort_by(|left, right| {
        left.role
            .cmp(&right.role)
            .then(left.harness.cmp(&right.harness))
    });
    Ok(resolved)
}

/// Resolve a source-controlled probe ID to fixed executable arguments.
///
/// The harness catalog cannot supply executable names or arguments. Supporting
/// a new live probe requires this code allowlist and its tests to change.
#[must_use]
pub fn trusted_version_probe(id: &str) -> Option<TrustedVersionProbe> {
    match id {
        "claude-cli-version" => Some(TrustedVersionProbe {
            command: "claude",
            args: &["--version"],
        }),
        "codex-cli-version" => Some(TrustedVersionProbe {
            command: "codex",
            args: &["--version"],
        }),
        "grok-cli-version" => Some(TrustedVersionProbe {
            command: "grok",
            args: &["--version"],
        }),
        _ => None,
    }
}

/// Load and validate every JSON binding record in `directory`.
///
/// A missing directory is an empty record set. Symlinks and files larger than
/// 1 MiB are rejected so doctor never follows a mutable indirection or parses
/// an unbounded local record.
///
/// # Errors
///
/// Returns an error naming the malformed record or catalog mismatch.
pub fn load_bindings(directory: &Path) -> Result<Vec<QualifiedBinding>, String> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        return Err(format!("{} is not a directory", directory.display()));
    }
    let catalog = harness_catalog()?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("read binding entry: {error}"))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut records = Vec::with_capacity(paths.len());
    let mut ids = BTreeSet::new();
    for path in paths {
        let data = read_bounded_record(&path)?;
        let record: QualifiedBinding = serde_json::from_slice(&data)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        validate_binding(&record, &catalog)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if !ids.insert(record.binding_id.clone()) {
            return Err(format!("duplicate binding id {}", record.binding_id));
        }
        records.push(record);
    }
    Ok(records)
}

fn read_bounded_record(path: &Path) -> Result<Vec<u8>, String> {
    read_bounded_json(path, "binding")
}

fn read_bounded_json(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let initial = std::fs::symlink_metadata(path)
        .map_err(|error| format!("stat {}: {error}", path.display()))?;
    if initial.file_type().is_symlink() {
        return Err(format!("refusing symlinked {label} {}", path.display()));
    }
    let file = open_without_reparse_follow(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("inspect open {}: {error}", path.display()))?;
    let current_file = open_without_reparse_follow(path)
        .map_err(|error| format!("reopen {}: {error}", path.display()))?;
    let current = std::fs::symlink_metadata(path)
        .map_err(|error| format!("stat {}: {error}", path.display()))?;
    if current.file_type().is_symlink() {
        return Err(format!("refusing symlinked {label} {}", path.display()));
    }
    if !opened.is_file() || !current.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    if !same_file_identity(&file, &opened, &current_file, &current) {
        return Err(format!(
            "{label} changed while opening; retry: {}",
            path.display()
        ));
    }
    if opened.len() > MAX_RECORD_BYTES {
        return Err(format!(
            "{label} exceeds {MAX_RECORD_BYTES} bytes: {}",
            path.display()
        ));
    }
    let mut data = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_RECORD_BYTES {
        return Err(format!(
            "{label} exceeds {MAX_RECORD_BYTES} bytes: {}",
            path.display()
        ));
    }
    Ok(data)
}

#[cfg(windows)]
fn open_without_reparse_follow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn open_without_reparse_follow(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

#[cfg(unix)]
fn same_file_identity(
    _opened_file: &std::fs::File,
    opened: &std::fs::Metadata,
    _current_file: &std::fs::File,
    current: &std::fs::Metadata,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    opened.dev() == current.dev() && opened.ino() == current.ino()
}

#[cfg(windows)]
fn same_file_identity(
    opened_file: &std::fs::File,
    _opened: &std::fs::Metadata,
    current_file: &std::fs::File,
    _current: &std::fs::Metadata,
) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    fn identity(file: &std::fs::File) -> Option<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` owns a live OS handle for the duration of the call,
        // and `info` is a valid writable output buffer of the required type.
        let succeeded =
            unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &raw mut info) };
        (succeeded != 0).then_some((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }

    identity(opened_file)
        .zip(identity(current_file))
        .is_some_and(|(opened, current)| opened == current)
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(
    _opened_file: &std::fs::File,
    opened: &std::fs::Metadata,
    _current_file: &std::fs::File,
    current: &std::fs::Metadata,
) -> bool {
    opened.len() == current.len() && opened.modified().ok() == current.modified().ok()
}

pub(crate) fn validate_binding(
    record: &QualifiedBinding,
    catalog: &BTreeMap<String, HarnessMetadata>,
) -> Result<(), String> {
    if record.schema_version != 1 {
        return Err(format!(
            "unsupported binding schema {}",
            record.schema_version
        ));
    }
    validate_safe_atom(&record.binding_id, "binding_id")?;
    for (value, label) in [
        (&record.provider, "provider"),
        (&record.lineage, "lineage"),
        (&record.qualified_at, "qualified_at"),
        (&record.requested.model, "requested.model"),
        (&record.requested.effort, "requested.effort"),
        (
            &record.requested.harness_version,
            "requested.harness_version",
        ),
        (&record.qualification.run_id, "qualification.run_id"),
        (
            &record.qualification.codeflow_revision,
            "qualification.codeflow_revision",
        ),
        (&record.approval.reviewer, "approval.reviewer"),
        (&record.approval.reviewed_at, "approval.reviewed_at"),
    ] {
        validate_nonempty(value, label)?;
    }
    validate_roles(&record.eligible_roles)?;
    let harness = catalog.get(&record.requested.harness).ok_or_else(|| {
        format!(
            "harness {} is not capability-supported",
            record.requested.harness
        )
    })?;
    if record.provider != harness.provider || record.lineage != harness.lineage {
        return Err("provider or lineage does not match the harness catalog".into());
    }
    if record.requested.model != record.observed.model
        || record.requested.effort != record.observed.effort
    {
        return Err("requested and observed model/effort differ".into());
    }
    validate_observed_evidence(&record.observed.evidence)?;
    validate_qualification(record)?;
    validate_settings_sources(&record.settings_sources)?;
    Ok(())
}

fn validate_roles(roles: &[String]) -> Result<(), String> {
    if roles.is_empty() {
        return Err("eligible_roles must not be empty".into());
    }
    if roles.iter().collect::<BTreeSet<_>>().len() != roles.len() {
        return Err("eligible_roles contains a duplicate".into());
    }
    let allowed_roles = [
        "primary",
        "producer",
        "reviewer",
        "evidence-worker",
        "claude-judgment-primary",
        "codex-engineering-primary",
        "grok-engineering-primary",
    ];
    if roles
        .iter()
        .any(|role| !allowed_roles.contains(&role.as_str()))
    {
        return Err("eligible_roles contains an unknown role".into());
    }
    Ok(())
}

fn validate_observed_evidence(evidence_items: &[ObservedEvidence]) -> Result<(), String> {
    if evidence_items.is_empty() {
        return Err("observed.evidence must not be empty".into());
    }
    for evidence in evidence_items {
        if !["session", "tool", "file", "command", "ui"].contains(&evidence.kind.as_str()) {
            return Err(format!("invalid observed evidence kind {}", evidence.kind));
        }
        validate_digest(&evidence.digest, "observed.evidence.digest")?;
    }
    Ok(())
}

fn validate_qualification(record: &QualifiedBinding) -> Result<(), String> {
    if record.qualification.suite != "full" {
        return Err("only full-suite qualifications may be recorded".into());
    }
    for (digest, label) in [
        (
            &record.requested.settings_digest,
            "requested.settings_digest",
        ),
        (
            &record.qualification.suite_digest,
            "qualification.suite_digest",
        ),
        (
            &record.qualification.result_digest,
            "qualification.result_digest",
        ),
    ] {
        validate_digest(digest, label)?;
    }
    if record.qualified_at != record.approval.reviewed_at {
        return Err("qualified_at and approval.reviewed_at differ".into());
    }
    validate_rfc3339(&record.qualified_at, "qualified_at")?;
    Ok(())
}

fn validate_settings_sources(settings_sources: &[SettingsSource]) -> Result<(), String> {
    let mut settings_paths = BTreeSet::new();
    for source in settings_sources {
        if !source.path.is_absolute() {
            return Err(format!(
                "settings source is not absolute: {}",
                source.path.display()
            ));
        }
        if !settings_paths.insert(&source.path) {
            return Err(format!(
                "duplicate settings source {}",
                source.path.display()
            ));
        }
        validate_digest(&source.digest, "settings_sources.digest")?;
    }
    Ok(())
}

fn validate_nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_safe_atom(value: &str, label: &str) -> Result<(), String> {
    validate_nonempty(value, label)?;
    if !value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
    {
        return Err(format!("{label} must start with a letter or digit"));
    }
    if value.chars().any(|character| {
        !(character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '.' | '_' | '-'))
    }) {
        return Err(format!("{label} contains unsafe characters"));
    }
    Ok(())
}

fn validate_digest(value: &str, label: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{label} is not canonical sha256"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} is not canonical sha256"));
    }
    Ok(())
}

fn validate_rfc3339(value: &str, label: &str) -> Result<(), String> {
    static EXPRESSION: OnceLock<regex::Regex> = OnceLock::new();
    let expression = EXPRESSION.get_or_init(|| {
        regex::Regex::new(
            r"^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})$",
        )
        .expect("static RFC 3339 expression")
    });
    let Some(captures) = expression.captures(value) else {
        return Err(format!("{label} must be RFC 3339"));
    };
    let number = |index: usize| {
        captures
            .get(index)
            .and_then(|capture| capture.as_str().parse::<u32>().ok())
            .ok_or_else(|| format!("{label} must be RFC 3339"))
    };
    let year = number(1)?;
    let month = number(2)?;
    let day = number(3)?;
    let hour = number(4)?;
    let minute = number(5)?;
    let second = number(6)?;
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => 0,
    };
    let timezone = captures
        .get(7)
        .map(|capture| capture.as_str())
        .ok_or_else(|| format!("{label} must be RFC 3339"))?;
    let timezone_valid = timezone == "Z"
        || timezone
            .get(1..3)
            .and_then(|hours| hours.parse::<u32>().ok())
            .zip(
                timezone
                    .get(4..6)
                    .and_then(|minutes| minutes.parse::<u32>().ok()),
            )
            .is_some_and(|(hours, minutes)| hours <= 23 && minutes <= 59);
    if year == 0
        || day == 0
        || day > days_in_month
        || hour > 23
        || minute > 59
        || second > 59
        || !timezone_valid
    {
        return Err(format!("{label} must be RFC 3339"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> String {
        format!("sha256:{}", "a".repeat(64))
    }

    fn record() -> QualifiedBinding {
        QualifiedBinding {
            schema_version: 1,
            binding_id: "fictional-orchid-high".into(),
            provider: "anthropic".into(),
            lineage: "claude".into(),
            eligible_roles: vec![
                "primary".into(),
                "reviewer".into(),
                "claude-judgment-primary".into(),
            ],
            qualified_at: "2026-07-25T10:00:00Z".into(),
            requested: RequestedBinding {
                model: "orchid-5".into(),
                effort: "high".into(),
                harness: "claude-code".into(),
                harness_version: "2.1.220".into(),
                settings_digest: digest(),
            },
            observed: ObservedBinding {
                model: "orchid-5".into(),
                effort: "high".into(),
                evidence: vec![ObservedEvidence {
                    kind: "session".into(),
                    digest: digest(),
                }],
            },
            qualification: QualificationEvidence {
                run_id: "run-1".into(),
                suite: "full".into(),
                suite_digest: digest(),
                result_digest: digest(),
                codeflow_revision: "abc123".into(),
            },
            settings_sources: Vec::new(),
            approval: Approval {
                reviewer: "operator".into(),
                reviewed_at: "2026-07-25T10:00:00Z".into(),
            },
        }
    }

    #[test]
    fn embedded_harnesses_satisfy_capability_contract() {
        let harnesses = harness_catalog().unwrap();
        assert_eq!(harnesses.len(), 4);
        assert!(harnesses.contains_key("claude-code"));
        assert!(harnesses.contains_key("codex-cli"));
        assert!(harnesses.contains_key("codex-app"));
        assert!(harnesses.contains_key("grok-cli"));
        assert_eq!(
            trusted_version_probe("claude-cli-version")
                .expect("Claude probe")
                .args,
            ["--version"]
        );
        assert!(trusted_version_probe("catalog-supplied-command").is_none());
    }

    #[test]
    fn embedded_ensemble_has_two_independent_stable_roles() {
        let ensemble = current_ensemble().unwrap();
        assert!(ensemble.len() >= 2);
        assert_eq!(ensemble["claude-judgment-primary"].seat, "claude-primary");
        assert_eq!(ensemble["codex-engineering-primary"].seat, "codex-primary");
        assert_eq!(ensemble["grok-engineering-primary"].seat, "grok-primary");
        assert_eq!(ensemble["grok-engineering-primary"].lineage, "grok");
        assert!(
            ensemble["claude-judgment-primary"]
                .internal_routes
                .iter()
                .any(|route| route.workloads.contains(&RouteWorkload::Reasoning)
                    && route.default_effort == RouteEffort::High),
            "Claude high in-family worker route missing"
        );
        assert!(
            ensemble["claude-judgment-primary"]
                .internal_routes
                .iter()
                .any(|route| route.workloads.contains(&RouteWorkload::Reasoning)
                    && route.default_effort == RouteEffort::Xhigh),
            "Claude xhigh in-family worker route missing"
        );
        assert!(
            ensemble["codex-engineering-primary"]
                .internal_routes
                .iter()
                .any(|route| route.workloads.contains(&RouteWorkload::Reasoning)
                    && route.default_effort == RouteEffort::High),
            "Codex high in-family worker route missing"
        );
        assert!(
            ensemble["codex-engineering-primary"]
                .internal_routes
                .iter()
                .any(|route| route.workloads.contains(&RouteWorkload::Reasoning)
                    && route.default_effort == RouteEffort::Xhigh),
            "Codex xhigh in-family worker route missing"
        );
        assert!(
            ensemble["grok-engineering-primary"]
                .internal_routes
                .iter()
                .any(|route| route.workloads.contains(&RouteWorkload::Reasoning)
                    && route.default_effort == RouteEffort::Xhigh),
            "Grok xhigh in-family worker route missing"
        );
        assert_ne!(
            ensemble["claude-judgment-primary"].lineage,
            ensemble["codex-engineering-primary"].lineage
        );
        assert!(trusted_version_probe("grok-cli-version").is_some());
    }

    fn write_selection(root: &Path, body: &serde_json::Value) {
        let directory = root.join(".codeflow");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("model-selection.json"),
            serde_json::to_vec_pretty(&body).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn absent_or_empty_project_selection_keeps_managed_defaults() {
        let root = tempfile::tempdir().unwrap();
        assert!(resolve_project_selection(root.path(), &[])
            .unwrap()
            .is_empty());
        write_selection(
            root.path(),
            &serde_json::json!({"schema_version": 1, "bindings": []}),
        );
        assert!(resolve_project_selection(root.path(), &[])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn project_selection_resolves_only_an_exact_qualified_role() {
        let root = tempfile::tempdir().unwrap();
        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [{
                    "role": "claude-judgment-primary",
                    "binding_id": "fictional-orchid-high"
                }]
            }),
        );
        let resolved = resolve_project_selection(root.path(), &[record()]).unwrap();
        assert_eq!(
            resolved,
            vec![ResolvedSelection {
                role: "claude-judgment-primary".into(),
                seat: "claude-primary".into(),
                binding_id: "fictional-orchid-high".into(),
                provider: "anthropic".into(),
                lineage: "claude".into(),
                harness: "claude-code".into(),
                model: "orchid-5".into(),
                effort: "high".into(),
            }]
        );
    }

    #[test]
    fn project_selection_is_atomic_for_missing_or_ineligible_bindings() {
        let root = tempfile::tempdir().unwrap();
        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [
                    {
                        "role": "claude-judgment-primary",
                        "binding_id": "fictional-orchid-high"
                    },
                    {
                        "role": "codex-engineering-primary",
                        "binding_id": "missing"
                    }
                ]
            }),
        );
        let error = resolve_project_selection(root.path(), &[record()]).unwrap_err();
        assert!(error.contains("missing binding missing"));

        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [{
                    "role": "codex-engineering-primary",
                    "binding_id": "fictional-orchid-high"
                }]
            }),
        );
        let error = resolve_project_selection(root.path(), &[record()]).unwrap_err();
        assert!(error.contains("not qualified for role"));
    }

    #[test]
    fn project_selection_rejects_provider_lineage_and_harness_mismatch() {
        let root = tempfile::tempdir().unwrap();
        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [{
                    "role": "codex-engineering-primary",
                    "binding_id": "fictional-orchid-high"
                }]
            }),
        );

        let mut wrong_lineage = record();
        wrong_lineage
            .eligible_roles
            .push("codex-engineering-primary".into());
        let error = resolve_project_selection(root.path(), &[wrong_lineage.clone()]).unwrap_err();
        assert!(error.contains("provider/lineage does not match"));

        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [{
                    "role": "claude-judgment-primary",
                    "binding_id": "fictional-orchid-high"
                }]
            }),
        );
        wrong_lineage.requested.harness = "codex-app".into();
        let error = resolve_project_selection(root.path(), &[wrong_lineage]).unwrap_err();
        assert!(error.contains("harness codex-app is not supported"));
    }

    #[test]
    fn pseudo_duo_selection_cannot_reuse_one_lineage_for_both_roles() {
        let root = tempfile::tempdir().unwrap();
        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [
                    {
                        "role": "claude-judgment-primary",
                        "binding_id": "fictional-orchid-high"
                    },
                    {
                        "role": "codex-engineering-primary",
                        "binding_id": "fictional-orchid-high"
                    }
                ]
            }),
        );
        let mut binding = record();
        binding
            .eligible_roles
            .push("codex-engineering-primary".into());
        let error = resolve_project_selection(root.path(), &[binding]).unwrap_err();
        assert!(
            error.contains("provider/lineage does not match"),
            "a pseudo-duo must fail before any partial selection is returned: {error}"
        );
    }

    #[test]
    fn project_selection_rejects_duplicate_role_harness_and_unknown_fields() {
        let root = tempfile::tempdir().unwrap();
        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [
                    {
                        "role": "claude-judgment-primary",
                        "binding_id": "fictional-orchid-high"
                    },
                    {
                        "role": "claude-judgment-primary",
                        "binding_id": "fictional-orchid-high"
                    }
                ]
            }),
        );
        let error = resolve_project_selection(root.path(), &[record()]).unwrap_err();
        assert!(error.contains("repeats role"));

        write_selection(
            root.path(),
            &serde_json::json!({
                "schema_version": 1,
                "bindings": [],
                "selector": "orchid"
            }),
        );
        let error = resolve_project_selection(root.path(), &[]).unwrap_err();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn project_selection_rejects_symlinks_and_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".codeflow");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("model-selection.json");
        let oversized = std::fs::File::create(&path).unwrap();
        oversized.set_len(MAX_RECORD_BYTES + 1).unwrap();
        let error = resolve_project_selection(root.path(), &[]).unwrap_err();
        assert!(error.contains("model selection exceeds"));

        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            let target = directory.join("target.json");
            std::fs::write(&target, b"{\"schema_version\":1,\"bindings\":[]}").unwrap();
            std::os::unix::fs::symlink(&target, &path).unwrap();
            let error = resolve_project_selection(root.path(), &[]).unwrap_err();
            assert!(error.contains("symlinked model selection"));

            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(directory.join("missing.json"), &path).unwrap();
            let error = resolve_project_selection(root.path(), &[]).unwrap_err();
            assert!(error.contains("symlinked model selection"));
        }
    }

    #[test]
    fn requested_observed_mismatch_is_rejected() {
        let mut binding = record();
        binding.observed.effort = "xhigh".into();
        let error = validate_binding(&binding, &harness_catalog().unwrap()).unwrap_err();
        assert!(error.contains("requested and observed"));
    }

    #[test]
    fn binding_id_must_start_with_an_alphanumeric_character() {
        let mut binding = record();
        binding.binding_id = ".fictional-orchid-high".into();
        let error = validate_binding(&binding, &harness_catalog().unwrap()).unwrap_err();
        assert!(error.contains("must start with a letter or digit"));
    }

    #[test]
    fn approval_timestamp_must_be_a_real_rfc3339_datetime() {
        let mut binding = record();
        binding.qualified_at = "2026-02-30T25:00:00Z".into();
        binding.approval.reviewed_at = binding.qualified_at.clone();
        let error = validate_binding(&binding, &harness_catalog().unwrap()).unwrap_err();
        assert!(error.contains("must be RFC 3339"));
    }

    #[test]
    fn approval_timestamp_rejects_unicode_numerals_without_panicking() {
        let mut binding = record();
        binding.qualified_at = "٢٠٢٦-٠٧-٢٥T١٠:٠٠:٠٠Z".into();
        binding.approval.reviewed_at = binding.qualified_at.clone();
        let error = validate_binding(&binding, &harness_catalog().unwrap()).unwrap_err();
        assert!(error.contains("must be RFC 3339"));
    }

    #[test]
    fn load_rejects_symlinked_record() {
        #[cfg(unix)]
        {
            let directory = tempfile::tempdir().unwrap();
            let target = directory.path().join("target");
            std::fs::write(&target, "{}").unwrap();
            std::os::unix::fs::symlink(&target, directory.path().join("binding.json")).unwrap();
            let error = load_bindings(directory.path()).unwrap_err();
            assert!(error.contains("symlinked"));
        }
    }

    #[test]
    fn load_bounds_record_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.json");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_RECORD_BYTES + 1).unwrap();
        let error = load_bindings(directory.path()).unwrap_err();
        assert!(error.contains("binding exceeds 1048576 bytes"));
    }

    fn shipped_routing_policy() -> RoutingPolicy {
        serde_json::from_str(ROUTING_POLICY).expect("shipped routing policy")
    }

    fn extra_family_review(invoke_when_available: bool) -> ExtraFamilyReview {
        ExtraFamilyReview {
            never_silent_vote: true,
            requires_named_assignment: true,
            invoke_when_available,
            triggers: vec!["security-sensitive change".into()],
            rule: "named evidence, never a silent third vote".into(),
        }
    }

    fn routing_policy(invoke_when_available: bool) -> RoutingPolicy {
        RoutingPolicy {
            schema_version: 1,
            policy_id: "claude-codex-duo".into(),
            default_review: "standing-pair".into(),
            design_production: "claude-native-session".into(),
            host_does_not_own_duty: true,
            extra_family_review: extra_family_review(invoke_when_available),
        }
    }

    #[test]
    fn shipped_routing_policy_invokes_extra_family_when_available() {
        let policy = shipped_routing_policy();
        validate_parsed_routing_policy(&policy, "claude-codex-duo").unwrap();
        assert!(policy.extra_family_review.invoke_when_available);
        assert!(policy.extra_family_review.never_silent_vote);
        assert!(policy.extra_family_review.requires_named_assignment);
        assert!(!policy.extra_family_review.triggers.is_empty());
    }

    #[test]
    fn routing_policy_rejects_invoke_when_available_false() {
        let error =
            validate_parsed_routing_policy(&routing_policy(false), "claude-codex-duo").unwrap_err();
        assert!(error.contains("invoked when available"));
    }

    #[test]
    fn routing_policy_rejects_silent_vote_and_unnamed_assignment() {
        let mut policy = routing_policy(true);
        policy.extra_family_review.never_silent_vote = false;
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo").is_err());
        policy.extra_family_review.never_silent_vote = true;
        policy.extra_family_review.requires_named_assignment = false;
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo").is_err());
        policy.extra_family_review.requires_named_assignment = true;
        policy.extra_family_review.triggers.clear();
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo").is_err());
        policy
            .extra_family_review
            .triggers
            .push("security-sensitive change".into());
        policy.extra_family_review.rule.clear();
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo").is_err());
    }

    #[test]
    fn routing_policy_rejects_schema_and_duty_drift() {
        let mut policy = routing_policy(true);
        policy.schema_version = 2;
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo")
            .unwrap_err()
            .contains("unsupported routing policy schema"));
        policy = routing_policy(true);
        policy.policy_id = "other".into();
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo")
            .unwrap_err()
            .contains("must match the current ensemble"));
        policy = routing_policy(true);
        policy.default_review = "all-qualified".into();
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo")
            .unwrap_err()
            .contains("standing-pair"));
        policy = routing_policy(true);
        policy.design_production = "host-drafts".into();
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo")
            .unwrap_err()
            .contains("Claude native-session"));
        policy = routing_policy(true);
        policy.host_does_not_own_duty = false;
        assert!(validate_parsed_routing_policy(&policy, "claude-codex-duo")
            .unwrap_err()
            .contains("host-does-not-own-duty"));
    }

    #[test]
    fn routing_policy_rejects_unknown_fields() {
        let error = serde_json::from_str::<RoutingPolicy>(
            r#"{"schema_version":1,"policy_id":"claude-codex-duo","default_review":"standing-pair","design_production":"claude-native-session","host_does_not_own_duty":true,"extra_family_review":{"never_silent_vote":true,"requires_named_assignment":true,"invoke_when_available":true,"triggers":["x"],"rule":"y","silent":true}}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    fn shipped_ensemble() -> EnsembleCatalog {
        serde_json::from_str(CURRENT_ENSEMBLE).expect("shipped ensemble")
    }

    fn binding_mut<'a>(ensemble: &'a mut EnsembleCatalog, role: &str) -> &'a mut EnsembleBinding {
        ensemble
            .bindings
            .iter_mut()
            .find(|binding| binding.role == role)
            .unwrap_or_else(|| panic!("missing role {role}"))
    }

    #[test]
    fn parsed_ensemble_rejects_schema_and_pair_drift() {
        let mut ensemble = shipped_ensemble();
        ensemble.schema_version = 3;
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("unsupported current ensemble schema"));

        let mut ensemble = shipped_ensemble();
        ensemble.standing_roles.pop();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("exactly two primary roles"));

        let mut ensemble = shipped_ensemble();
        ensemble.bindings.truncate(1);
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("must include the standing pair"));

        let mut ensemble = shipped_ensemble();
        ensemble.standing_roles[1] = ensemble.standing_roles[0].clone();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("duplicate standing role"));

        let mut ensemble = shipped_ensemble();
        ensemble.standing_roles[1] = "grok-engineering-primary".into();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("must remain claude-judgment-primary"));
    }

    #[test]
    fn parsed_ensemble_rejects_binding_and_trigger_defects() {
        let mut ensemble = shipped_ensemble();
        ensemble.high_triggers.clear();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("effort triggers and rules must not be empty"));

        let mut ensemble = shipped_ensemble();
        ensemble.bindings[1].seat = ensemble.bindings[0].seat.clone();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("duplicate ensemble seat"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "claude-judgment-primary")
            .native_selectors
            .clear();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("has no selector or responsibility"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "claude-judgment-primary").internal_routes[0]
            .efforts
            .clear();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("has no effort"));

        let mut ensemble = shipped_ensemble();
        ensemble
            .bindings
            .retain(|binding| binding.role != "claude-judgment-primary");
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("standing role claude-judgment-primary is missing"));

        let mut ensemble = shipped_ensemble();
        let claude_lineage = binding_mut(&mut ensemble, "claude-judgment-primary")
            .lineage
            .clone();
        binding_mut(&mut ensemble, "grok-engineering-primary").lineage = claude_lineage;
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("primary lineage"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "grok-engineering-primary")
            .native_selectors
            .insert("not-a-harness".into(), "x".into());
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("unsupported harness"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "grok-engineering-primary").provider = "anthropic".into();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("mismatches harness"));
    }

    #[test]
    fn parsed_ensemble_rejects_unknown_and_untyped_route_values() {
        let mut unknown_catalog: serde_json::Value =
            serde_json::from_str(CURRENT_ENSEMBLE).unwrap();
        unknown_catalog["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<EnsembleCatalog>(unknown_catalog)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));

        let mut unknown_route: serde_json::Value = serde_json::from_str(CURRENT_ENSEMBLE).unwrap();
        unknown_route["bindings"][0]["internal_routes"][0]["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<EnsembleCatalog>(unknown_route)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));

        for (field, invalid) in [
            ("efforts", serde_json::json!(["medium-or-high"])),
            ("workloads", serde_json::json!(["design"])),
            ("status", serde_json::json!("enabled")),
        ] {
            let mut ensemble: serde_json::Value = serde_json::from_str(CURRENT_ENSEMBLE).unwrap();
            ensemble["bindings"][0]["internal_routes"][0][field] = invalid;
            assert!(
                serde_json::from_value::<EnsembleCatalog>(ensemble).is_err(),
                "invalid route field {field} parsed"
            );
        }
    }

    #[test]
    fn parsed_ensemble_validates_typed_route_shape() {
        let mut ensemble = shipped_ensemble();
        let binding = binding_mut(&mut ensemble, "codex-engineering-primary");
        let primary_selector = binding.native_selectors["codex-cli"].clone();
        let route = &mut binding.internal_routes[0];
        route.native_selectors.clear();
        route
            .native_selectors
            .insert("codex-cli".into(), primary_selector);
        route.efforts = vec![RouteEffort::High];
        route.default_effort = RouteEffort::High;
        route.evidence.clear();
        assert!(validate_parsed_ensemble(ensemble).is_ok());

        for invalid in ["", "-leading", "UPPER", "unsafe/route"] {
            let mut ensemble = shipped_ensemble();
            binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0].route_id =
                invalid.into();
            assert!(
                validate_parsed_ensemble(ensemble).is_err(),
                "unsafe route id accepted: {invalid:?}"
            );
        }

        let mut ensemble = shipped_ensemble();
        let binding = binding_mut(&mut ensemble, "codex-engineering-primary");
        binding.internal_routes[1].route_id = binding.internal_routes[0].route_id.clone();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("duplicate internal route"));

        let mut ensemble = shipped_ensemble();
        let route = &mut binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0];
        route.efforts.push(route.efforts[0]);
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("duplicate effort"));

        let mut ensemble = shipped_ensemble();
        let route = &mut binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0];
        route.default_effort = RouteEffort::Low;
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("default effort is not in efforts"));

        let mut ensemble = shipped_ensemble();
        let route = &mut binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0];
        route.workloads.push(route.workloads[0]);
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("duplicate workload"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0]
            .workloads
            .clear();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("has no workload"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0]
            .native_selectors
            .clear();
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("has no native selector"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0]
            .native_selectors
            .insert("codex-cli".into(), String::new());
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("native selector must not be empty"));

        let mut ensemble = shipped_ensemble();
        binding_mut(&mut ensemble, "codex-engineering-primary").internal_routes[0]
            .native_selectors
            .insert("claude-code".into(), "orchid".into());
        assert!(validate_parsed_ensemble(ensemble)
            .unwrap_err()
            .contains("outside parent role"));
    }

    #[test]
    fn parsed_ensemble_keeps_design_candidates_owner_bound() {
        let mut candidate = shipped_ensemble();
        let route = binding_mut(&mut candidate, "claude-judgment-primary")
            .internal_routes
            .iter()
            .find(|route| {
                route
                    .workloads
                    .contains(&RouteWorkload::DesignImplementation)
            })
            .expect("shipped design qualification candidate");
        assert_eq!(route.status, RouteStatus::Candidate);
        assert!(route.evidence.is_empty());
        assert!(
            validate_parsed_ensemble(candidate).is_ok(),
            "owner-bound candidate design routes must be representable for disposable qualification"
        );

        let mut scoped = shipped_ensemble();
        let route = binding_mut(&mut scoped, "claude-judgment-primary")
            .internal_routes
            .iter_mut()
            .find(|route| {
                route
                    .workloads
                    .contains(&RouteWorkload::DesignImplementation)
            })
            .expect("shipped design qualification candidate");
        route.status = RouteStatus::ScopedQualified;
        route.evidence = vec!["docs/verification/orchid-design-pilot.md".into()];
        assert!(
            validate_parsed_ensemble(scoped).is_ok(),
            "owner-bound scoped-qualified design routes with evidence must remain valid"
        );

        for status in [RouteStatus::Candidate, RouteStatus::ScopedQualified] {
            let mut wrong_owner = shipped_ensemble();
            let route =
                &mut binding_mut(&mut wrong_owner, "codex-engineering-primary").internal_routes[0];
            route.workloads.push(RouteWorkload::DesignImplementation);
            route.status = status;
            if status == RouteStatus::ScopedQualified {
                route.evidence = vec!["docs/verification/wrong-owner.md".into()];
            }
            assert!(validate_parsed_ensemble(wrong_owner)
                .unwrap_err()
                .contains("outside design_execution_owner"));
        }

        let mut non_standing = shipped_ensemble();
        non_standing.design_execution_owner = "grok-engineering-primary".into();
        assert!(validate_parsed_ensemble(non_standing)
            .unwrap_err()
            .contains("must be a standing role"));

        let mut wrong_lineage = shipped_ensemble();
        wrong_lineage.design_execution_owner = "codex-engineering-primary".into();
        for route in &mut binding_mut(&mut wrong_lineage, "claude-judgment-primary").internal_routes
        {
            if route
                .workloads
                .contains(&RouteWorkload::DesignImplementation)
            {
                route.workloads = vec![RouteWorkload::Evidence];
            }
        }
        assert!(validate_parsed_ensemble(wrong_lineage)
            .unwrap_err()
            .contains("must have Claude lineage"));
    }

    #[test]
    fn parsed_ensemble_requires_safe_scoped_evidence() {
        let mut missing = shipped_ensemble();
        binding_mut(&mut missing, "claude-judgment-primary").internal_routes[0].status =
            RouteStatus::ScopedQualified;
        assert!(validate_parsed_ensemble(missing)
            .unwrap_err()
            .contains("has no evidence"));

        let mut valid = shipped_ensemble();
        let route = &mut binding_mut(&mut valid, "claude-judgment-primary").internal_routes[0];
        route.status = RouteStatus::ScopedQualified;
        route.evidence = vec!["docs/verification/orchid-high-reasoning.md".into()];
        assert!(validate_parsed_ensemble(valid).is_ok());

        for invalid in [
            "",
            "/tmp/result.md",
            "C:/tmp/result.md",
            r"\\server\share\result.md",
            "docs/../result.md",
            "docs//result.md",
            "https://example.com/result.md",
        ] {
            let mut ensemble = shipped_ensemble();
            let route =
                &mut binding_mut(&mut ensemble, "claude-judgment-primary").internal_routes[0];
            route.status = RouteStatus::ScopedQualified;
            route.evidence = vec![invalid.into()];
            assert!(
                validate_parsed_ensemble(ensemble)
                    .unwrap_err()
                    .contains("portable repository-relative path"),
                "unsafe evidence path accepted: {invalid:?}"
            );
        }

        let mut duplicate = shipped_ensemble();
        let route = &mut binding_mut(&mut duplicate, "claude-judgment-primary").internal_routes[0];
        route.status = RouteStatus::ScopedQualified;
        route.evidence = vec![
            "docs/verification/a.md".into(),
            "docs/verification/a.md".into(),
        ];
        assert!(validate_parsed_ensemble(duplicate)
            .unwrap_err()
            .contains("duplicate evidence"));
    }
}
