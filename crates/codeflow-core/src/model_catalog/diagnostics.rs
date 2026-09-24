use std::collections::BTreeMap;

use super::{CatalogInputs, Effort, Lifecycle, ResolveRequest};

impl CatalogInputs {
    /// Illustrative host/duty matrix and advisory catalog/canary warnings.
    /// No native command is executed and no availability is inferred.
    ///
    /// # Errors
    /// Returns a catalog or resolver validation failure.
    pub fn diagnostic_report(&self) -> Result<(String, bool), String> {
        let mut rows = vec!["illustrative: context-free, not a task's resolution".to_owned()];
        let empty = BTreeMap::new();
        for family in &self.catalog.families {
            for host in &family.harnesses {
                for duty in self.catalog.duties.keys() {
                    let result = self.catalog.resolve(&ResolveRequest {
                        duty,
                        task: "",
                        host_harness: host,
                        author_lineage: None,
                        exclusions: &[],
                        observed_ids: &empty,
                        trigger_facts: &[],
                        requested_override: None,
                        operator_override: None,
                    })?;
                    let state = if result.is_open() { "open" } else { "filled" };
                    rows.push(format!(
                        "{host} {duty}: {state}; {}",
                        serde_json::to_string(&result).map_err(|e| e.to_string())?
                    ));
                }
            }
        }
        let mut warnings = Vec::new();
        for line in &self.catalog.lines {
            let adopted = line
                .versions
                .iter()
                .position(|v| v.id == line.adopted_version)
                .unwrap_or(0);
            for (index, version) in line.versions.iter().enumerate() {
                if index > adopted
                    && version.lifecycle == Lifecycle::Active
                    && version.designations.is_empty()
                    && line.adoption == super::Adoption::Manual
                {
                    warnings.push(format!(
                        "{}: newer version not yet designated or adopted",
                        version.id
                    ));
                }
                for designation in &version.designations {
                    let seat = self
                        .catalog
                        .seat(&designation.seat)
                        .ok_or("unknown designation seat")?;
                    for (harness, selector) in &version.selectors {
                        if self
                            .catalog
                            .full_binding(seat, version, harness, selector, Effort::High)
                            .is_none()
                        {
                            warnings.push(format!(
                                "{} {} {harness}: designated version with no full-suite record",
                                version.id, seat.id
                            ));
                        }
                    }
                }
            }
        }
        for (pinned, observed) in &self.canary_observations {
            if pinned != observed {
                warnings.push(format!("canary identity drift: {pinned} observed {observed}; recorded observation, not a live probe"));
            }
        }
        let warned = !warnings.is_empty();
        rows.extend(
            warnings
                .into_iter()
                .map(|warning| format!("warning: {warning}")),
        );
        rows.push("doctor did not launch a model".into());
        Ok((rows.join("\n"), warned))
    }
}
