use std::collections::BTreeMap;

use super::{AuthorityRole, CatalogInputs, Effort, Lifecycle, ResolveRequest};

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
                        area: None,
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
        rows.extend(self.project_authority_rows());
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

impl CatalogInputs {
    /// The project's design authority and standing reviews as the working
    /// tree states them. Design authority takes effect for a task only once
    /// committed on its integration target; standing reviews apply as read.
    fn project_authority_rows(&self) -> Vec<String> {
        let Some(selection) = &self.catalog.selection else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if let Some(authority) = &selection.design_authority {
            let lines = authority
                .lines
                .iter()
                .map(|(line, entry)| {
                    let role = match entry.role {
                        AuthorityRole::Owner => "owner",
                        AuthorityRole::CoOwner => "co-owner",
                        AuthorityRole::Consultant => "consultant",
                    };
                    let mut parts = vec![format!("{line} {role}")];
                    if let Some(approves) = &entry.approves {
                        parts.push(format!("approves at {}", approves.join(", ")));
                    }
                    if let Some(before) = &entry.consulted_before {
                        parts.push(format!("consulted before {}", before.join(", ")));
                    }
                    parts.join(", ")
                })
                .collect::<Vec<_>>()
                .join("; ");
            rows.push(format!(
                "project design authority: seat {}: {lines}; designated {}, {}; applies to a task \
                 once committed on its integration target; a same-family design approval is \
                 never the independent review",
                authority.seat, authority.designated.date, authority.designated.record
            ));
        }
        for review in selection.standing_reviews.iter().flatten() {
            rows.push(format!("project standing review: {}", review.disposition()));
        }
        rows
    }
}
