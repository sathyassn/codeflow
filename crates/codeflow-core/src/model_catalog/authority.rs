//! Project design authority and standing reviews (`.codeflow/model-selection.json`
//! schema 2, issue 43).
//!
//! The block narrows the managed catalog; it never extends it. It names only
//! lines the design owner seat already lists, in that seat's own family, and
//! adds no selector, command, family, harness or designation. A standing
//! review adds a catalog participant the duty already routes; it never
//! removes or replaces one. Design authority is read from the committed file
//! on the task's integration target, never from the working tree.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::validate::nonempty;
use super::{Catalog, Effort, Lifecycle, Target};

/// The engine-owned duty for a same-family design approval.
pub const DESIGN_APPROVAL: &str = "design-approval";

/// Only the duties whose extra-family participant a standing review can add.
pub(crate) const STANDING_REVIEW_DUTIES: [&str; 2] = ["unit-review", "body-review"];

/// Who holds design authority inside the design owner seat.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignAuthority {
    pub seat: String,
    pub designated: AuthorityRecord,
    pub lines: BTreeMap<String, LineAuthority>,
}

/// The operator instruction that designated the arrangement.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRecord {
    pub date: String,
    pub record: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineAuthority {
    pub role: AuthorityRole,
    #[serde(default)]
    pub approves: Option<Vec<String>>,
    #[serde(default)]
    pub consulted_before: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityRole {
    Owner,
    CoOwner,
    Consultant,
}

/// A standing extra-family review for one area.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingReview {
    pub area: String,
    pub seat: String,
    pub mode: ReviewMode,
    pub per: ReviewPer,
    pub record: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewMode {
    ReadOnly,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewPer {
    Unit,
    Phase,
}

impl ReviewPer {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Phase => "phase",
        }
    }
}

/// Design authority as committed on a task's integration target.
#[derive(Debug, Clone)]
pub struct AnchoredAuthority {
    pub authority: DesignAuthority,
    /// Where the committed block was read, for the resolution's limitation.
    pub source: String,
}

fn date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

fn labels(values: Option<&Vec<String>>, label: &str) -> Result<(), String> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.is_empty() {
        return Err(format!("design authority {label} must not be empty"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(value, &format!("design authority {label} entry"))?;
        if !seen.insert(value) {
            return Err(format!("duplicate design authority {label} entry {value}"));
        }
    }
    Ok(())
}

impl DesignAuthority {
    /// Validate against the managed catalog. Any error rejects the whole file.
    ///
    /// # Errors
    /// Names the first line, family, order, version or record violation.
    pub fn validate(&self, catalog: &Catalog) -> Result<(), String> {
        if self.seat != catalog.design_owner {
            return Err("design authority seat must be the catalog's design owner seat".into());
        }
        let seat = catalog
            .seat(&self.seat)
            .ok_or("unknown design owner seat")?;
        if !date(&self.designated.date) {
            return Err("design authority date must be YYYY-MM-DD".into());
        }
        nonempty(&self.designated.record, "design authority record")?;
        let mut owners = Vec::new();
        let mut others = 0;
        for (id, entry) in &self.lines {
            let line = catalog
                .line(id)
                .ok_or_else(|| format!("design authority names unknown line {id}"))?;
            if line.family != seat.family {
                return Err(format!(
                    "design authority line {id} is another family; another family designs only \
                     through a task-specific OPERATOR_OVERRIDE"
                ));
            }
            if !seat.lines.contains(id) {
                return Err(format!(
                    "design authority line {id} is not listed by seat {}",
                    seat.id
                ));
            }
            if !line.versions.iter().any(|v| {
                !v.personal_candidate
                    && v.lifecycle == Lifecycle::Active
                    && v.efforts.contains(&Effort::High)
                    && (v.designations.iter().any(|d| d.seat == seat.id)
                        || v.selectors.iter().any(|(h, s)| {
                            catalog.full_binding(seat, v, h, s, Effort::High).is_some()
                        }))
            }) {
                return Err(format!(
                    "design authority line {id} has no active version designated or fully \
                     qualified for seat {}",
                    seat.id
                ));
            }
            match entry.role {
                AuthorityRole::Owner => {
                    if entry.approves.is_some() || entry.consulted_before.is_some() {
                        return Err(
                            "the design owner line takes no approves or consulted_before".into(),
                        );
                    }
                    owners.push(id);
                }
                AuthorityRole::CoOwner | AuthorityRole::Consultant => others += 1,
            }
            labels(entry.approves.as_ref(), "approves")?;
            labels(entry.consulted_before.as_ref(), "consulted_before")?;
        }
        if owners.len() != 1 {
            return Err("design authority needs exactly one owner line".into());
        }
        if seat.lines.first() != Some(owners[0]) {
            return Err(format!(
                "design authority owner {} must be seat {}'s first line",
                owners[0], seat.id
            ));
        }
        if others == 0 {
            return Err("design authority names no co-owner or consultant".into());
        }
        Ok(())
    }

    pub(crate) fn role(&self, line: &str) -> Option<AuthorityRole> {
        self.lines.get(line).map(|entry| entry.role)
    }
}

impl StandingReview {
    /// The triggered participant of `duty` that this review adds, if any.
    pub(crate) fn participant<'a>(
        &self,
        catalog: &'a Catalog,
        duty: &str,
    ) -> Option<&'a super::TriggeredParticipant> {
        catalog.duties.get(duty)?.triggered.iter().find(|t| {
            t.participant
                .alternatives
                .iter()
                .any(|a| a.target == Target::Seat(self.seat.clone()))
        })
    }

    pub(crate) fn disposition(&self) -> String {
        format!(
            "standing assignment (area {}, seat {}, read-only, per {}; {})",
            self.area,
            self.seat,
            self.per.as_str(),
            self.record
        )
    }
}

/// Validate every standing review. Any error rejects the whole file.
///
/// # Errors
/// Names the first unknown seat, standing-pair seat, unrouted seat or record gap.
pub fn validate_standing_reviews(
    reviews: &[StandingReview],
    catalog: &Catalog,
) -> Result<(), String> {
    if reviews.is_empty() {
        return Err("standing_reviews must not be empty when present".into());
    }
    let mut seen = BTreeSet::new();
    for review in reviews {
        nonempty(&review.area, "standing review area")?;
        nonempty(&review.record, "standing review record")?;
        let seat = catalog
            .seat(&review.seat)
            .ok_or_else(|| format!("standing review names unknown seat {}", review.seat))?;
        if catalog.standing_seats.contains(&seat.id) {
            return Err(format!(
                "standing review seat {} is a standing-pair seat, not an extra family",
                seat.id
            ));
        }
        if !STANDING_REVIEW_DUTIES
            .iter()
            .any(|duty| review.participant(catalog, duty).is_some())
        {
            return Err(format!(
                "standing review seat {} is not a triggered review participant in the catalog",
                seat.id
            ));
        }
        if !seen.insert((&review.area, &review.seat)) {
            return Err(format!(
                "duplicate standing review for area {} and seat {}",
                review.area, review.seat
            ));
        }
    }
    Ok(())
}

/// Resolution text for a designated line on a design-authority duty.
pub(crate) fn designation_text(role: AuthorityRole, anchored: &AnchoredAuthority) -> String {
    let role = match role {
        AuthorityRole::Owner => "design owner",
        AuthorityRole::CoOwner => "design co-owner",
        AuthorityRole::Consultant => "design consultant",
    };
    format!(
        "{role} by project designation (.codeflow/model-selection.json, {}; {})",
        anchored.authority.designated.record, anchored.source
    )
}
