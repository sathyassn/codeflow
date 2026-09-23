use std::collections::BTreeSet;

use serde::Deserialize;

use super::{Catalog, Effort};
use crate::model_qualification::{
    harness_catalog, validate_binding, QualifiedBinding, ResolvedSelection,
};

/// ADR-0041 schema 1 references only. No repository selectors or effort edits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSelection {
    pub schema_version: u64,
    pub bindings: Vec<BindingReference>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingReference {
    pub role: String,
    pub binding_id: String,
}

impl ProjectSelection {
    /// Parse schema 1 references without silently accepting duplicate JSON keys.
    ///
    /// # Errors
    /// Rejects raw selectors, unknown fields, duplicate keys and other schemas.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let selection: Self =
            crate::strict_json::parse_strict_json(bytes).map_err(|e| e.to_string())?;
        if selection.schema_version != 1 {
            return Err("unsupported project model-selection schema".into());
        }
        Ok(selection)
    }

    /// Validate the entire selection before returning the exact role/harness.
    ///
    /// # Errors
    /// Rejects invalid records, role/lineage mismatches and effort reuse.
    pub fn resolve(
        &self,
        catalog: &Catalog,
        records: &[QualifiedBinding],
        role: &str,
        harness: &str,
        effort: Effort,
    ) -> Result<Option<ResolvedSelection>, String> {
        catalog.validate()?;
        if self.schema_version != 1 {
            return Err("unsupported project model-selection schema".into());
        }
        let harnesses = harness_catalog()?;
        let mut ids = BTreeSet::new();
        for record in records {
            validate_binding(record, &harnesses)?;
            if !ids.insert(&record.binding_id) {
                return Err("duplicate binding record".into());
            }
        }
        let mut tuples = BTreeSet::new();
        let mut resolved = None;
        let mut role_seen = false;
        for entry in &self.bindings {
            let seat = catalog
                .seats
                .iter()
                .find(|s| s.role == entry.role)
                .ok_or("unknown selection role")?;
            let family = catalog.family(&seat.family).ok_or("unknown seat family")?;
            let record = records
                .iter()
                .find(|r| r.binding_id == entry.binding_id)
                .ok_or("missing binding record")?;
            if !record.eligible_roles.contains(&entry.role)
                || record.lineage != family.lineage
                || record.provider != family.provider
            {
                return Err("binding does not qualify the exact role and lineage".into());
            }
            if !family.harnesses.contains(&record.requested.harness) {
                return Err("binding harness is unsupported for role".into());
            }
            if !tuples.insert((&entry.role, &record.requested.harness)) {
                return Err("duplicate role/harness selection".into());
            }
            if entry.role == role {
                role_seen = true;
                if record.requested.harness == harness {
                    if record.requested.effort != effort.as_str() || effort < Effort::High {
                        return Err("project binding effort mismatch or below seat floor".into());
                    }
                    resolved = Some(ResolvedSelection {
                        role: entry.role.clone(),
                        seat: seat.id.clone(),
                        binding_id: entry.binding_id.clone(),
                        provider: record.provider.clone(),
                        lineage: record.lineage.clone(),
                        harness: record.requested.harness.clone(),
                        model: record.observed.model.clone(),
                        effort: record.requested.effort.clone(),
                    });
                }
            }
        }
        if role_seen && resolved.is_none() {
            return Err("project binding does not apply to another harness".into());
        }
        Ok(resolved)
    }
}
