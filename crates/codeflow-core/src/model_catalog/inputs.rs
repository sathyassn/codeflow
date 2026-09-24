use std::collections::BTreeMap;
use std::path::Path;

use super::{Catalog, CatalogDocument, Effort, Exclusion, PersonalOverlay, ProjectSelection};

const EMBEDDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/base/agents/skills/cf-model-orchestrator/resources/current-ensemble.json"
));
const RESOURCE: &str = "skills/cf-model-orchestrator/resources/current-ensemble.json";

/// Read the installed managed catalog, falling back to the shipped catalog.
///
/// # Errors
/// Rejects unreadable or invalid catalogs; never falls back past invalid data.
pub fn load_catalog_document(root: &Path) -> Result<CatalogDocument, String> {
    for prefix in [".agents", ".claude"] {
        if let Some(bytes) = optional_file(&root.join(prefix).join(RESOURCE))? {
            return CatalogDocument::parse(&bytes);
        }
    }
    CatalogDocument::parse(EMBEDDED)
}

pub(super) fn optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
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
    /// `home` is the user-owned `CodeFlow` directory, not the project directory.
    ///
    /// # Errors
    /// An invalid input rejects the entire configuration without partial use.
    pub fn load(mut catalog: Catalog, root: &Path, home: Option<&Path>) -> Result<Self, String> {
        let mut exclusions = Vec::new();
        let mut canary_observations = BTreeMap::new();
        if let Some(home) = home {
            if let Some(bytes) = optional_file(&home.join("model-catalog.local.json"))? {
                (catalog, exclusions) = PersonalOverlay::parse(&bytes)?.apply(&catalog)?;
            }
            catalog
                .bindings
                .extend(crate::model_qualification::load_bindings(
                    &home.join("qualified-bindings"),
                )?);
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
        catalog.validate()?;
        if let Some(bytes) = optional_file(&root.join(".codeflow/model-selection.json"))? {
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
