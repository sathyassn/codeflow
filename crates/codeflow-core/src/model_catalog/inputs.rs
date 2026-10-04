use std::collections::BTreeMap;
use std::path::Path;

use super::{AnchoredAuthority, Catalog, Effort, Exclusion, PersonalOverlay, ProjectSelection};

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
    match std::fs::symlink_metadata(&path) {
        Ok(_) => crate::model_qualification::read_bounded_json(&path, "model selection").map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("stat {}: {error}", path.display())),
    }
}

pub(super) fn optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

/// Validate the whole selection before any duty resolves. An unrelated bad
/// entry must not be ignored, and nothing is partially applied.
fn validate_selection(selection: &ProjectSelection, catalog: &Catalog) -> Result<(), String> {
    // Validate even an empty selection, and every selected tuple before
    // resolving any duty.
    selection.resolve(catalog, &catalog.bindings, "", "", Effort::High)?;
    for entry in &selection.bindings {
        let record = catalog
            .bindings
            .iter()
            .find(|b| b.binding_id == entry.binding_id)
            .ok_or("missing binding record")?;
        let effort: Effort =
            serde_json::from_value(serde_json::Value::String(record.requested.effort.clone()))
                .map_err(|e| e.to_string())?;
        selection.resolve(
            catalog,
            &catalog.bindings,
            &entry.role,
            &record.requested.harness,
            effort,
        )?;
    }
    selection.validate_blocks(catalog)
}

const SELECTION: &str = ".codeflow/model-selection.json";

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
            validate_selection(&selection, &catalog)?;
            catalog.selection = Some(selection);
        }
        Ok(Self {
            catalog,
            exclusions,
            canary_observations,
        })
    }
}

impl CatalogInputs {
    /// Apply the design-authority block as committed on `task`'s integration
    /// target (the merge-base of `HEAD` and that target). The working-tree
    /// file never confers design authority; when it carries a block that
    /// is not applied, the reason is kept for the resolution to report.
    ///
    /// # Errors
    /// Rejects an invalid committed file as a whole.
    pub fn anchor_design_authority(
        &mut self,
        root: &Path,
        task: Option<&str>,
    ) -> Result<(), String> {
        let working = self
            .catalog
            .selection
            .as_ref()
            .is_some_and(|s| s.design_authority.is_some());
        let Some(task) = task else {
            if working {
                self.catalog.authority_note = Some(
                    "the project design-authority block applies only with --task, read as \
                     committed on the task's integration target"
                        .into(),
                );
            }
            return Ok(());
        };
        let anchored =
            match crate::workgraph::work_start::anchored_project_file(root, task, SELECTION) {
                Ok(anchored) => anchored,
                Err(reason) => {
                    if working {
                        self.catalog.authority_note =
                            Some(format!("project design authority not applied: {reason}"));
                    }
                    return Ok(());
                }
            };
        let short = &anchored.base[..anchored.base.len().min(9)];
        let committed = match &anchored.content {
            Some(bytes) => {
                let selection = ProjectSelection::parse(bytes)
                    .and_then(|selection| {
                        validate_selection(&selection, &self.catalog)?;
                        Ok(selection)
                    })
                    .map_err(|e| {
                        format!(
                            "{SELECTION} committed on {} at {short}: {e}",
                            anchored.target
                        )
                    })?;
                selection.design_authority
            }
            None => None,
        };
        match committed {
            Some(authority) => {
                self.catalog.authority = Some(AnchoredAuthority {
                    authority,
                    source: format!("committed on {} at {short}", anchored.target),
                });
            }
            None if working => {
                self.catalog.authority_note = Some(format!(
                    "the working-tree design-authority block is not committed on {} at {short}; \
                     design stays with the design owner's first line",
                    anchored.target
                ));
            }
            None => {}
        }
        Ok(())
    }
}
