use std::collections::BTreeSet;

use super::{Catalog, Effort, Lifecycle, Participant, Target, Version};
use crate::model_qualification::{
    harness_catalog, trusted_version_probe, validate_binding,
    validate_repository_relative_reference,
};

pub(super) fn nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn unique<'a>(values: impl Iterator<Item = &'a str>, label: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(value, label)?;
        if !seen.insert(value) {
            return Err(format!("duplicate {label}: {value}"));
        }
    }
    Ok(())
}

pub(super) fn floor(duty: &str, seat: bool) -> Effort {
    if duty == "xhigh-reasoning" {
        Effort::Xhigh
    } else if seat
        || !matches!(
            duty,
            "bounded-execution"
                | "light-execution"
                | "evidence-collection"
                | "engineering-implementation"
                | "design-implementation"
        )
    {
        Effort::High
    } else {
        Effort::Medium
    }
}

impl Catalog {
    /// Validate catalog references, trust boundaries and effort floors.
    ///
    /// # Errors
    /// Rejects unsupported native metadata and contradictory routing policy.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 5 {
            return Err(format!(
                "unsupported model catalog schema {}",
                self.schema_version
            ));
        }
        nonempty(&self.policy_id, "policy_id")?;
        unique(self.families.iter().map(|f| f.id.as_str()), "family")?;
        unique(self.lines.iter().map(|l| l.id.as_str()), "line")?;
        unique(self.seats.iter().map(|s| s.id.as_str()), "seat")?;
        unique(self.seats.iter().map(|s| s.role.as_str()), "role")?;
        unique(
            self.seats.iter().map(|s| s.family.as_str()),
            "primary family",
        )?;
        unique(
            self.families.iter().map(|f| f.lineage.as_str()),
            "family lineage",
        )?;
        let harnesses = harness_catalog()?;
        for family in &self.families {
            nonempty(&family.usage_bucket, "usage_bucket")?;
            if family.harnesses.is_empty() {
                return Err(format!("family {} has no harness", family.id));
            }
            unique(
                family.harnesses.iter().map(String::as_str),
                "family harness",
            )?;
            unique(family.probes.iter().map(String::as_str), "family probe")?;
            for id in &family.harnesses {
                let harness = harnesses
                    .get(id)
                    .ok_or_else(|| format!("unsupported harness {id}"))?;
                if harness.lineage != family.lineage || harness.provider != family.provider {
                    return Err(format!("family {} mismatches harness {id}", family.id));
                }
            }
            for probe in &family.probes {
                if trusted_version_probe(probe).is_none()
                    || !family
                        .harnesses
                        .iter()
                        .any(|id| harnesses[id].version_probe.as_ref() == Some(probe))
                {
                    return Err(format!("unsupported probe {probe} for {}", family.id));
                }
            }
            if !self.seats.iter().any(|seat| seat.family == family.id) {
                return Err(format!("family {} needs one primary seat", family.id));
            }
        }
        for binding in &self.bindings {
            validate_binding(binding, &harnesses)?;
        }
        unique(
            self.bindings.iter().map(|b| b.binding_id.as_str()),
            "binding",
        )?;
        self.validate_seats()?;
        let mut ids = BTreeSet::new();
        let mut pins = BTreeSet::new();
        for line in &self.lines {
            let family = self
                .family(&line.family)
                .ok_or_else(|| format!("unknown family {}", line.family))?;
            if !line
                .versions
                .iter()
                .any(|version| version.id == line.adopted_version)
            {
                return Err(format!("line {} has no adopted version", line.id));
            }
            for version in &line.versions {
                nonempty(&version.id, "version id")?;
                nonempty(&version.pinned_id, "pinned id")?;
                nonempty(&version.alias, "alias")?;
                if !ids.insert(&version.id) || !pins.insert(&version.pinned_id) {
                    return Err(format!("duplicate version or pinned id {}", version.id));
                }
                if version.selectors.is_empty() || version.efforts.is_empty() {
                    return Err(format!(
                        "version {} needs selectors and efforts",
                        version.id
                    ));
                }
                unique(version.efforts.iter().map(|e| e.as_str()), "effort")?;
                for (harness, selector) in &version.selectors {
                    nonempty(selector, "selector")?;
                    if !family.harnesses.contains(harness) {
                        return Err(format!("unsupported harness {harness} for {}", version.id));
                    }
                }
                self.validate_evidence(&line.id, version)?;
            }
        }
        self.validate_duties()
    }

    fn validate_seats(&self) -> Result<(), String> {
        let standing: BTreeSet<_> = self.standing_seats.iter().collect();
        if self.standing_seats.len() != 2 || standing.len() != 2 {
            return Err("standing seats must be two distinct lineages".into());
        }
        let mut lineages = BTreeSet::new();
        for id in &self.standing_seats {
            let seat = self
                .seat(id)
                .ok_or_else(|| format!("unknown standing seat {id}"))?;
            let family = self.family(&seat.family).ok_or("unknown standing family")?;
            lineages.insert(&family.lineage);
        }
        if lineages.len() != 2 {
            return Err("standing seats must be two distinct lineages".into());
        }
        let owner = self
            .seat(&self.design_owner)
            .ok_or("unknown design owner")?;
        if !self.standing_seats.contains(&owner.id)
            || self
                .family(&owner.family)
                .is_none_or(|f| f.lineage != "claude")
        {
            return Err("design owner must be a standing Claude seat".into());
        }
        for seat in &self.seats {
            if self.line(&seat.id).is_some() {
                return Err(format!("seat id {} collides with a line id", seat.id));
            }
            if self.family(&seat.family).is_none() || seat.lines.is_empty() {
                return Err(format!("seat {} has no family or lines", seat.id));
            }
            unique(seat.lines.iter().map(String::as_str), "seat line")?;
            for (index, id) in seat.lines.iter().enumerate() {
                let line = self
                    .line(id)
                    .ok_or_else(|| format!("unknown seat line {id}"))?;
                if line.family != seat.family {
                    return Err(format!("seat {} line {id} has another family", seat.id));
                }
                if !line.versions.iter().any(|v| {
                    !v.personal_candidate
                        && v.lifecycle != Lifecycle::Retired
                        && (v.lifecycle != Lifecycle::FallbackOnly || index > 0)
                        && v.efforts.contains(&Effort::High)
                        && (v.designations.iter().any(|d| d.seat == seat.id)
                            || v.selectors.iter().any(|(h, s)| {
                                self.full_binding(seat, v, h, s, Effort::High).is_some()
                            }))
                }) {
                    return Err(format!(
                        "seat {} line {id} has no potentially eligible version",
                        seat.id
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_evidence(&self, line: &str, version: &Version) -> Result<(), String> {
        unique(
            version.designations.iter().map(|d| d.seat.as_str()),
            "designation",
        )?;
        for designation in &version.designations {
            nonempty(&designation.record, "designation record")?;
            // Dates are data, not a runtime freshness claim.
            let date = &designation.date;
            if date.len() != 10
                || !date.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
            {
                return Err("designation date must be YYYY-MM-DD".into());
            }
            if version.lifecycle == Lifecycle::Retired {
                return Err("designation on retired version".into());
            }
            if self
                .seat(&designation.seat)
                .is_none_or(|seat| !seat.lines.iter().any(|id| id == line))
            {
                return Err("designation line is not listed by its seat".into());
            }
        }
        for evidence in &version.qualification {
            nonempty(&evidence.record, "evidence record")?;
            nonempty(&evidence.duty, "evidence duty")?;
            if evidence.evidence.is_empty() {
                return Err("scoped evidence needs evidence paths".into());
            }
            for path in &evidence.evidence {
                validate_repository_relative_reference(path, "scoped evidence path")?;
            }
            if version.selectors.get(&evidence.harness) != Some(&evidence.selector)
                || !version.efforts.contains(&evidence.effort)
            {
                return Err("scoped evidence tuple does not match version".into());
            }
        }
        Ok(())
    }

    fn validate_duties(&self) -> Result<(), String> {
        if self.high_triggers.is_empty() || self.xhigh_triggers.is_empty() || self.rules.is_empty()
        {
            return Err("catalog needs effort triggers and rules".into());
        }
        let native_triggers = crate::model_qualification::routing_policy_triggers()?;
        for (name, facts) in &self.named_policies {
            nonempty(name, "named policy")?;
            if facts.is_empty() {
                return Err("named policy needs trigger facts".into());
            }
            unique(facts.iter().map(String::as_str), "policy fact")?;
        }
        for (id, duty) in &self.duties {
            nonempty(id, "duty")?;
            if id == super::DESIGN_APPROVAL {
                return Err(format!(
                    "duty {id} is engine-owned and comes from the project design authority"
                ));
            }
            if duty.required.is_empty() {
                return Err(format!("duty {id} has no required participant"));
            }
            unique(
                duty.required
                    .iter()
                    .chain(duty.triggered.iter().map(|t| &t.participant))
                    .map(|p| p.id.as_str()),
                "participant",
            )?;
            for participant in &duty.required {
                self.validate_participant(id, participant)?;
            }
            for triggered in &duty.triggered {
                if !self.named_policies.contains_key(&triggered.trigger)
                    && !native_triggers.contains(&triggered.trigger)
                {
                    return Err(format!("unknown participant trigger {}", triggered.trigger));
                }
                self.validate_participant(id, &triggered.participant)?;
            }
        }
        Ok(())
    }

    fn validate_participant(&self, duty: &str, participant: &Participant) -> Result<(), String> {
        if participant.alternatives.is_empty() {
            return Err("participant has no alternatives".into());
        }
        for alternative in &participant.alternatives {
            let family = match &alternative.target {
                Target::Seat(id) => Some(
                    &self
                        .seat(id)
                        .ok_or_else(|| format!("unknown seat {id}"))?
                        .family,
                ),
                Target::Line(id) => Some(
                    &self
                        .line(id)
                        .ok_or_else(|| format!("unknown line {id}"))?
                        .family,
                ),
                Target::HostSeat => None,
            };
            if let Some(harness) = &alternative.harness {
                if !self
                    .families
                    .iter()
                    .any(|f| family.is_none_or(|id| *id == f.id) && f.harnesses.contains(harness))
                {
                    return Err(format!("unsupported alternative harness {harness}"));
                }
            }
            if alternative.effort < floor(duty, !matches!(alternative.target, Target::Line(_))) {
                return Err(format!("duty {duty} default below its effort floor"));
            }
            if duty == "design" && alternative.target != Target::Seat(self.design_owner.clone()) {
                return Err("design must use the design owner seat".into());
            }
        }
        Ok(())
    }
}
