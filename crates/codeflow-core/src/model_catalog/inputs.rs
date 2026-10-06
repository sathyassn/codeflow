use std::collections::BTreeMap;
use std::path::Path;

use super::{Catalog, Effort, Exclusion, PersonalOverlay, ProjectSelection};

const EMBEDDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json"
));
const RESOURCE: &str = "skills/cf-model-orchestrator/resources/current-ensemble.json";

/// Read the installed managed catalog, falling back to the shipped catalog.
///
/// # Errors
/// Rejects unreadable or invalid catalogs, including any schema other than 5;
/// never falls back past invalid data.
pub fn load_catalog(root: &Path) -> Result<Catalog, String> {
    for prefix in [".agents", ".claude"] {
        let path = root.join(prefix).join(RESOURCE);
        if let Some(bytes) = optional_file(&path)? {
            return Catalog::parse(&bytes).map_err(|error| {
                let schema = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| value["schema_version"].as_u64());
                if schema.is_some_and(|version| version < 5) {
                    format!(
                        "{}: installed model catalog predates schema 5; run `codeflow update`",
                        path.display()
                    )
                } else {
                    error
                }
            });
        }
    }
    Catalog::parse(EMBEDDED)
}

/// Repository content: refuse symlinks and oversized files like binding records.
fn project_selection(root: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = root.join(".codeflow/model-selection.json");
    if crate::absence::proven_absent(&path)
        .map_err(|error| format!("stat {}: {error}", path.display()))?
    {
        return Ok(None);
    }
    crate::model_qualification::read_bounded_json(&path, "model selection").map(Some)
}

pub(super) fn optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    if crate::absence::proven_absent(path)
        .map_err(|error| format!("stat {}: {error}", path.display()))?
    {
        return Ok(None);
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| format!("read {}: {error}", path.display()))
}

/// Validated, read-only inputs shared by resolution and diagnostics.
pub struct CatalogInputs {
    pub catalog: Catalog,
    pub exclusions: Vec<Exclusion>,
    /// Recorded canary observations are diagnostic only. Resolution uses the
    /// caller's fresh `--observed` facts, never these potentially stale facts.
    pub canary_observations: BTreeMap<String, String>,
}

impl CatalogInputs {
    /// Load local additions, exclusions, binding records and project references.
    /// `home` is the user-owned `CodeFlow` directory (overlay and canary record),
    /// not the project directory. `bindings` is the qualified-binding record
    /// directory, passed explicitly so its location is never inferred from home.
    ///
    /// # Errors
    /// An invalid input rejects the entire configuration without partial use.
    pub fn load(
        mut catalog: Catalog,
        root: &Path,
        home: Option<&Path>,
        bindings: Option<&Path>,
    ) -> Result<Self, String> {
        let mut exclusions = Vec::new();
        let mut canary_observations = BTreeMap::new();
        if let Some(home) = home {
            if let Some(bytes) = optional_file(&home.join("model-catalog.local.json"))? {
                (catalog, exclusions) = PersonalOverlay::parse(&bytes)?.apply(&catalog)?;
            }
            if let Some(bytes) = optional_file(&home.join("model-canary.json"))? {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Canary {
                    schema_version: u64,
                    observed_ids: BTreeMap<String, String>,
                }
                let canary: Canary = crate::strict_json::parse_strict_json(&bytes)
                    .map_err(|e| format!("invalid model canary: {e}"))?;
                if canary.schema_version != 1
                    || canary
                        .observed_ids
                        .iter()
                        .any(|(a, b)| a.trim().is_empty() || b.trim().is_empty())
                {
                    return Err("invalid model canary schema or identity".into());
                }
                canary_observations = canary.observed_ids;
            }
        }
        if let Some(directory) = bindings {
            catalog
                .bindings
                .extend(crate::model_qualification::load_bindings(directory)?);
        }
        catalog.validate()?;
        if let Some(bytes) = project_selection(root)? {
            let selection = ProjectSelection::parse(&bytes)?;
            // Validate even an empty selection, and every selected tuple before
            // resolving any duty. An unrelated bad entry must not be ignored.
            selection.resolve(&catalog, &catalog.bindings, "", "", Effort::High)?;
            for entry in &selection.bindings {
                let record = catalog
                    .bindings
                    .iter()
                    .find(|b| b.binding_id == entry.binding_id)
                    .ok_or("missing binding record")?;
                let effort: Effort = serde_json::from_value(serde_json::Value::String(
                    record.requested.effort.clone(),
                ))
                .map_err(|e| e.to_string())?;
                selection.resolve(
                    &catalog,
                    &catalog.bindings,
                    &entry.role,
                    &record.requested.harness,
                    effort,
                )?;
            }
            catalog.selection = Some(selection);
        }
        Ok(Self {
            catalog,
            exclusions,
            canary_observations,
        })
    }
}

#[cfg(all(test, unix))]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_optional_catalog_inputs_require_proven_absence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("optional.json");
        assert!(optional_file(&path).unwrap().is_none());
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(optional_file(&path).is_err());
        assert!(optional_file(&path.join("child")).is_err());
    }
    #[test]
    fn r22_project_selection_requires_proven_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_selection(dir.path()).unwrap().is_none());
        std::os::unix::fs::symlink("missing", dir.path().join(".codeflow")).unwrap();
        assert!(project_selection(dir.path()).is_err());
    }
}
