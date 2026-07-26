//! Human-approved model+harness qualification records and project selection.
//!
//! Qualification records are evidence indexes, not runtime routing
//! configuration. A project may reference those records by stable role, but
//! neither surface launches a harness or accepts executable configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

const MAX_RECORD_BYTES: u64 = 1024 * 1024;
const HARNESS_CATALOG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-evaluate-model/resources/harnesses.json"
));
const CURRENT_ENSEMBLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json"
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
    bindings: Vec<EnsembleBinding>,
    xhigh_triggers: Vec<String>,
    rules: Vec<String>,
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
    pub model_class: String,
    pub effort: String,
    #[serde(rename = "use")]
    pub use_case: String,
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedBinding {
    pub model: String,
    pub effort: String,
    pub harness: String,
    pub harness_version: String,
    pub settings_digest: String,
}

/// Native observation proving the requested model and effort.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedBinding {
    pub model: String,
    pub effort: String,
    pub evidence: Vec<ObservedEvidence>,
}

/// Content-addressed observation retained without potentially sensitive refs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidence {
    pub kind: String,
    pub digest: String,
}

/// Full-suite result that justified promotion.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationEvidence {
    pub run_id: String,
    pub suite: String,
    pub suite_digest: String,
    pub result_digest: String,
    pub codeflow_revision: String,
}

/// Optional live settings input whose digest doctor can recompute.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSource {
    pub path: PathBuf,
    pub digest: String,
}

/// Explicit human approval retained from the full evaluation result.
#[derive(Debug, Clone, Deserialize)]
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

/// Parse and validate the fully managed current ensemble.
///
/// # Errors
///
/// Returns an error when roles, seats, lineages, selectors, or required
/// responsibility fields are missing or contradictory.
pub fn current_ensemble() -> Result<BTreeMap<String, EnsembleBinding>, String> {
    let ensemble: EnsembleCatalog =
        serde_json::from_str(CURRENT_ENSEMBLE).map_err(|error| error.to_string())?;
    if ensemble.schema_version != 2 {
        return Err(format!(
            "unsupported current ensemble schema {}",
            ensemble.schema_version
        ));
    }
    validate_nonempty(&ensemble.policy_id, "ensemble.policy_id")?;
    if ensemble.bindings.len() != 2 {
        return Err("the current duo must define exactly two primary roles".into());
    }
    if ensemble.xhigh_triggers.is_empty() || ensemble.rules.is_empty() {
        return Err("current ensemble effort triggers and rules must not be empty".into());
    }
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
        for route in &binding.internal_routes {
            validate_nonempty(&route.model_class, "internal route model_class")?;
            validate_nonempty(&route.effort, "internal route effort")?;
            validate_nonempty(&route.use_case, "internal route use")?;
        }
        let role = binding.role.clone();
        if roles.insert(role.clone(), binding).is_some() {
            return Err(format!("duplicate ensemble role {role}"));
        }
    }
    Ok(roles)
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

fn validate_binding(
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
            binding_id: "claude-fable-high".into(),
            provider: "anthropic".into(),
            lineage: "claude".into(),
            eligible_roles: vec![
                "primary".into(),
                "reviewer".into(),
                "claude-judgment-primary".into(),
            ],
            qualified_at: "2026-07-25T10:00:00Z".into(),
            requested: RequestedBinding {
                model: "fable-5".into(),
                effort: "high".into(),
                harness: "claude-code".into(),
                harness_version: "2.1.220".into(),
                settings_digest: digest(),
            },
            observed: ObservedBinding {
                model: "fable-5".into(),
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
        assert_eq!(harnesses.len(), 3);
        assert!(harnesses.contains_key("claude-code"));
        assert!(harnesses.contains_key("codex-cli"));
        assert!(harnesses.contains_key("codex-app"));
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
        assert_eq!(ensemble.len(), 2);
        assert_eq!(ensemble["claude-judgment-primary"].seat, "claude-primary");
        assert_eq!(ensemble["codex-engineering-primary"].seat, "codex-primary");
        assert_ne!(
            ensemble["claude-judgment-primary"].lineage,
            ensemble["codex-engineering-primary"].lineage
        );
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
                    "binding_id": "claude-fable-high"
                }]
            }),
        );
        let resolved = resolve_project_selection(root.path(), &[record()]).unwrap();
        assert_eq!(
            resolved,
            vec![ResolvedSelection {
                role: "claude-judgment-primary".into(),
                seat: "claude-primary".into(),
                binding_id: "claude-fable-high".into(),
                provider: "anthropic".into(),
                lineage: "claude".into(),
                harness: "claude-code".into(),
                model: "fable-5".into(),
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
                        "binding_id": "claude-fable-high"
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
                    "binding_id": "claude-fable-high"
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
                    "binding_id": "claude-fable-high"
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
                    "binding_id": "claude-fable-high"
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
                        "binding_id": "claude-fable-high"
                    },
                    {
                        "role": "codex-engineering-primary",
                        "binding_id": "claude-fable-high"
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
                        "binding_id": "claude-fable-high"
                    },
                    {
                        "role": "claude-judgment-primary",
                        "binding_id": "claude-fable-high"
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
                "selector": "opus"
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
        binding.binding_id = ".claude-fable-high".into();
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
}
