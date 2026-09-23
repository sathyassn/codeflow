use serde::Deserialize;

use super::{Catalog, Exclusion, ExclusionScope, Lifecycle, Version};

/// A personal overlay cannot carry designation, evidence, or new routes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalOverlay {
    pub schema_version: u64,
    pub additions: Vec<CandidateAddition>,
    pub exclusions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAddition {
    pub line: String,
    pub id: String,
    pub alias: String,
    pub pinned_id: String,
    pub selectors: std::collections::BTreeMap<String, String>,
    pub efforts: Vec<super::Effort>,
}

impl PersonalOverlay {
    /// Parse an overlay atomically, without applying it.
    ///
    /// # Errors
    /// Rejects duplicate keys, unknown fields and unsupported schemas.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let overlay: Self =
            crate::strict_json::parse_strict_json(bytes).map_err(|e| e.to_string())?;
        if overlay.schema_version != 1 {
            return Err("unsupported personal overlay schema".into());
        }
        Ok(overlay)
    }

    /// Return a validated copy and explicit exclusions; never mutate the input.
    ///
    /// # Errors
    /// Any invalid addition or exclusion rejects the whole overlay.
    pub fn apply(self, catalog: &Catalog) -> Result<(Catalog, Vec<Exclusion>), String> {
        if self.schema_version != 1 {
            return Err("unsupported personal overlay schema".into());
        }
        let mut copy = catalog.clone();
        for addition in self.additions {
            let line = copy
                .lines
                .iter_mut()
                .find(|l| l.id == addition.line)
                .ok_or("overlay names unknown line")?;
            line.versions.push(Version {
                id: addition.id,
                alias: addition.alias,
                pinned_id: addition.pinned_id,
                selectors: addition.selectors,
                efforts: addition.efforts,
                lifecycle: Lifecycle::Active,
                designations: Vec::new(),
                qualification: Vec::new(),
                personal_candidate: true,
            });
        }
        copy.validate()?;
        let mut exclusions = Vec::new();
        for id in self.exclusions {
            if !copy
                .lines
                .iter()
                .flat_map(|l| &l.versions)
                .any(|v| v.id == id || v.pinned_id == id)
            {
                return Err(format!("overlay excludes unknown version {id}"));
            }
            // Explicit operator-local exclusions are effective independently
            // of native availability. The return is ready for ResolveRequest.
            exclusions.push(Exclusion {
                scope: ExclusionScope::Version(id),
                reason: "personal overlay exclusion".into(),
                fresh_native: true,
            });
        }
        Ok((copy, exclusions))
    }
}
