use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    validate::{floor, nonempty},
    Adoption, Alternative, Catalog, Effort, Family, Lifecycle, LineageRelation, Participant,
    ParticipantLabel, ProductLine, Seat, Target, Version,
};
use crate::model_qualification::QualifiedBinding;

/// Only fresh, native exclusions are effective. Unknown usage is not exclusion.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    pub scope: ExclusionScope,
    pub reason: String,
    pub fresh_native: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum ExclusionScope {
    Version(String),
    Harness(String),
    UsageBucket(String),
}

/// Already parsed from an approved Plan vN task record by the caller.
/// This type provides matching, not authentication or plan anchoring.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorOverride {
    pub task: String,
    pub duty: String,
    pub route: Alternative,
    pub plan_version: String,
    pub instruction_record: String,
}

/// All contextual facts are explicit; no ambient time, IO, or availability.
#[derive(Debug, Clone)]
pub struct ResolveRequest<'a> {
    pub duty: &'a str,
    pub task: &'a str,
    pub host_harness: &'a str,
    pub author_lineage: Option<&'a str>,
    pub exclusions: &'a [Exclusion],
    pub observed_ids: &'a BTreeMap<String, String>,
    pub trigger_facts: &'a [String],
    /// Invocation route to match against the anchored operator record.
    pub requested_override: Option<&'a Alternative>,
    pub operator_override: Option<&'a OperatorOverride>,
}

/// Eligibility applies to a single version and exact execution tuple.
#[derive(Debug, Clone)]
pub struct EligibilityRequest<'a> {
    pub duty: &'a str,
    pub line: &'a str,
    pub version: &'a str,
    pub seat: Option<&'a str>,
    pub harness: &'a str,
    pub effort: Effort,
    pub exclusions: &'a [Exclusion],
    pub observed_ids: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Eligibility {
    Designated,
    Qualified,
    ScopedQualified,
    Candidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedParticipant {
    pub participant: String,
    pub label: ParticipantLabel,
    pub seat: Option<String>,
    pub line: String,
    pub version: String,
    pub lineage: String,
    /// The only launch identity. Never the floating alias.
    pub pinned_id: String,
    pub requested_selector: String,
    pub harness: String,
    pub effort: Effort,
    pub eligibility: Eligibility,
    pub reduced_assurance: bool,
    pub limitations: Vec<String>,
    pub remaining_alternatives: Vec<ResolvedAlternative>,
    pub operator_override: Option<OperatorOverride>,
    /// Workers return to this seat, which retains approval.
    pub approval_owner: Option<String>,
}

/// An eligible remaining launch, including alternatives inside the same seat.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAlternative {
    pub seat: Option<String>,
    pub line: String,
    pub version: String,
    pub pinned_id: String,
    pub harness: String,
    pub effort: Effort,
    pub eligibility: Eligibility,
}

impl From<ResolvedParticipant> for ResolvedAlternative {
    fn from(value: ResolvedParticipant) -> Self {
        Self {
            seat: value.seat,
            line: value.line,
            version: value.version,
            pinned_id: value.pinned_id,
            harness: value.harness,
            effort: value.effort,
            eligibility: value.eligibility,
        }
    }
}

#[derive(Default)]
struct Candidates {
    eligible: Vec<ResolvedParticipant>,
    drifted: Vec<ResolvedParticipant>,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenParticipant {
    pub participant: String,
    pub label: ParticipantLabel,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Resolution {
    pub participants: Vec<ResolvedParticipant>,
    pub obligations: Vec<ResolvedParticipant>,
    /// Unfilled participants, including labeled optional second opinions.
    pub open: Vec<OpenParticipant>,
    pub xhigh_trigger_met: bool,
}

impl Resolution {
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
            .iter()
            .any(|gap| gap.label == ParticipantLabel::Required)
    }
}

/// The only sanctioned way to fill `design` without the owner's first line.
const DESIGN_OVERRIDE_PATH: &str = "design waits for the design owner's first line; a \
    committed OPERATOR_OVERRIDE block (task, duty, route, effort, plan, instruction) in the \
    task record's Execution contract may name another seat line for that task";

fn candidate_duty(duty: &str) -> bool {
    matches!(
        duty,
        "engineering-implementation"
            | "bounded-execution"
            | "light-execution"
            | "evidence-collection"
            | "reasoning-support"
            | "xhigh-reasoning"
            | "consultation"
            | "general-review"
            | "design-and-editorial-review"
    )
}

impl Catalog {
    pub(super) fn full_binding(
        &self,
        seat: &Seat,
        version: &Version,
        harness: &str,
        selector: &str,
        effort: Effort,
    ) -> Option<&QualifiedBinding> {
        let family = self.family(&seat.family)?;
        self.bindings.iter().find(|binding| {
            binding.eligible_roles.contains(&seat.role)
                && binding.provider == family.provider
                && binding.lineage == family.lineage
                && binding.requested.harness == harness
                && binding.requested.model == selector
                && binding.observed.model == version.pinned_id
                && binding.requested.effort == effort.as_str()
                && binding.observed.effort == effort.as_str()
        })
    }

    /// Check lifecycle, authority, tuple, exclusions and native identity.
    ///
    /// # Errors
    /// Names the failed eligibility condition. Drift never inherits evidence.
    pub fn eligible(&self, request: &EligibilityRequest<'_>) -> Result<Eligibility, String> {
        let line = self.line(request.line).ok_or("unknown line")?;
        let version = line
            .versions
            .iter()
            .find(|v| v.id == request.version)
            .ok_or("unknown version")?;
        let family = self.family(&line.family).ok_or("unknown family")?;
        let seat = request
            .seat
            .map(|id| self.seat(id).ok_or("unknown seat"))
            .transpose()?;
        if let Some(seat) = seat {
            if version.personal_candidate {
                return Err("personal overlay candidates never fill seats".into());
            }
            if !seat.lines.contains(&line.id) {
                return Err("line not listed by seat".into());
            }
            if request.duty == "design"
                && (seat.id != self.design_owner || seat.lines.first() != Some(&line.id))
            {
                return Err(DESIGN_OVERRIDE_PATH.into());
            }
        } else if matches!(
            request.duty,
            "design" | "direction-approval" | "fidelity-approval"
        ) {
            return Err("workers never hold direction or fidelity approval".into());
        }
        Self::check_native(version, family, request)?;
        if version.lifecycle == Lifecycle::FallbackOnly
            && !seat.is_some_and(|s| {
                s.lines
                    .iter()
                    .position(|id| id == &line.id)
                    .is_some_and(|i| i > 0)
            })
        {
            return Err("fallback-only version requires a listed seat fallback".into());
        }
        if request
            .observed_ids
            .get(&version.pinned_id)
            .is_some_and(|id| id != &version.pinned_id)
        {
            return Err("identity drift inherits no designation or evidence".into());
        }
        let selector = &version.selectors[request.harness];
        if let Some(seat) = seat {
            if let Some(selection) = &self.selection {
                if let Some(selected) = selection.resolve(
                    self,
                    &self.bindings,
                    &seat.role,
                    request.harness,
                    request.effort,
                )? {
                    if selected.model != version.pinned_id {
                        return Err("version differs from project binding selection".into());
                    }
                }
            }
            if self
                .full_binding(seat, version, request.harness, selector, request.effort)
                .is_some()
            {
                return Ok(Eligibility::Qualified);
            }
            if version.designations.iter().any(|d| d.seat == seat.id) {
                return Ok(Eligibility::Designated);
            }
            return Err("seat needs its designation or an exact full-suite role binding".into());
        }
        if version.qualification.iter().any(|e| {
            e.harness == request.harness
                && e.selector == *selector
                && e.effort == request.effort
                && e.duty == request.duty
        }) {
            return Ok(Eligibility::ScopedQualified);
        }
        if candidate_duty(request.duty) {
            Ok(Eligibility::Candidate)
        } else {
            Err("worker duty requires scoped evidence for the exact tuple".into())
        }
    }

    fn check_native(
        version: &Version,
        family: &Family,
        request: &EligibilityRequest<'_>,
    ) -> Result<(), String> {
        if version.lifecycle == Lifecycle::Retired {
            return Err("retired version".into());
        }
        if !version.selectors.contains_key(request.harness) {
            return Err("unsupported harness".into());
        }
        if !version.efforts.contains(&request.effort) {
            return Err("unsupported effort; never clamped".into());
        }
        if request.effort < floor(request.duty, request.seat.is_some()) {
            return Err("effort below duty floor".into());
        }
        for exclusion in request.exclusions.iter().filter(|e| e.fresh_native) {
            let applies = match &exclusion.scope {
                ExclusionScope::Version(id) => {
                    id == &version.id
                        || id == &version.pinned_id
                        || request.observed_ids.get(&version.pinned_id) == Some(id)
                }
                ExclusionScope::Harness(id) => id == request.harness,
                ExclusionScope::UsageBucket(id) => id == &family.usage_bucket,
            };
            if applies {
                return Err(format!("native exclusion: {}", exclusion.reason));
            }
        }
        Ok(())
    }

    /// Return all owed participants and obligations, including named open gaps.
    ///
    /// # Errors
    /// Returns catalog validation errors or rejects an override on a non-design duty.
    pub fn resolve(&self, request: &ResolveRequest<'_>) -> Result<Resolution, String> {
        self.validate()?;
        if request.duty != "design"
            && (request.operator_override.is_some() || request.requested_override.is_some())
        {
            return Err("OPERATOR_OVERRIDE is only valid for design".into());
        }
        let mut result = Resolution::default();
        if request.duty == "test-authoring" {
            result.open.push(OpenParticipant {
                participant: "test-authoring".into(),
                label: ParticipantLabel::Required,
                reasons: vec![
                    "not resolved separately; the unit's executing participant writes its tests"
                        .into(),
                ],
            });
            return Ok(result);
        }
        let Some(duty) = self.duties.get(request.duty) else {
            result.open.push(OpenParticipant {
                participant: request.duty.into(),
                label: ParticipantLabel::Required,
                reasons: vec!["unknown duty".into()],
            });
            return Ok(result);
        };
        for participant in &duty.required {
            self.resolve_participant(participant, request, &mut result);
        }
        for triggered in &duty.triggered {
            if triggered
                .unless_author
                .as_deref()
                .is_some_and(|lineage| Some(lineage) == request.author_lineage)
            {
                continue;
            }
            if self.triggered(&triggered.trigger, request.trigger_facts) {
                self.resolve_participant(&triggered.participant, request, &mut result);
            }
        }
        if self
            .xhigh_triggers
            .iter()
            .any(|t| request.trigger_facts.contains(t))
        {
            self.add_xhigh(request, &mut result);
        }
        Ok(result)
    }

    fn triggered(&self, trigger: &str, facts: &[String]) -> bool {
        self.named_policies.get(trigger).map_or_else(
            || facts.iter().any(|f| f == trigger),
            |triggers| triggers.iter().any(|f| facts.contains(f)),
        )
    }

    fn host_seat(&self, harness: &str) -> Option<&Seat> {
        let family = self
            .families
            .iter()
            .find(|f| f.harnesses.iter().any(|h| h == harness))?;
        self.seats.iter().find(|seat| seat.family == family.id)
    }

    fn relation_matches(
        &self,
        relation: LineageRelation,
        family: &Family,
        request: &ResolveRequest<'_>,
    ) -> bool {
        match relation {
            LineageRelation::Any => true,
            LineageRelation::OppositeAuthor => request
                .author_lineage
                .is_some_and(|author| author != family.lineage),
            LineageRelation::SameAuthor => request.author_lineage == Some(family.lineage.as_str()),
            LineageRelation::SameHost => self
                .host_seat(request.host_harness)
                .is_some_and(|s| s.family == family.id),
        }
    }

    fn route_lines<'a>(
        &'a self,
        alternative: &Alternative,
        request: &ResolveRequest<'_>,
    ) -> Vec<(Option<&'a Seat>, &'a ProductLine)> {
        let seat = match &alternative.target {
            Target::Seat(id) => self.seat(id),
            Target::HostSeat => self.host_seat(request.host_harness),
            Target::Line(id) => {
                return self
                    .line(id)
                    .map(|line| vec![(None, line)])
                    .unwrap_or_default()
            }
        };
        seat.map(|seat| {
            seat.lines
                .iter()
                .take(if request.duty == "design" {
                    1
                } else {
                    usize::MAX
                })
                .filter_map(|id| self.line(id).map(|line| (Some(seat), line)))
                .collect()
        })
        .unwrap_or_default()
    }

    fn resolve_participant(
        &self,
        participant: &Participant,
        request: &ResolveRequest<'_>,
        result: &mut Resolution,
    ) {
        if request.duty == "design"
            && (request.operator_override.is_some() || request.requested_override.is_some())
        {
            match self.resolve_override(participant, request) {
                Ok(chosen) => result.participants.push(chosen),
                Err(reason) => result.open.push(OpenParticipant {
                    participant: participant.id.clone(),
                    label: participant.label,
                    reasons: vec![reason],
                }),
            }
            return;
        }
        let mut candidates = Candidates::default();
        for alternative in &participant.alternatives {
            self.collect_alternative(participant, alternative, request, &mut candidates);
        }
        let mut eligible = candidates.eligible.into_iter().chain(candidates.drifted);
        if let Some(mut chosen) = eligible.next() {
            chosen.remaining_alternatives = eligible.map(ResolvedAlternative::from).collect();
            result.participants.push(chosen);
        } else {
            if candidates.reasons.is_empty() {
                candidates
                    .reasons
                    .push("no eligible version at the adopted effort".into());
            }
            if request.duty == "design" {
                candidates.reasons.push(DESIGN_OVERRIDE_PATH.into());
            }
            result.open.push(OpenParticipant {
                participant: participant.id.clone(),
                label: participant.label,
                reasons: candidates.reasons,
            });
        }
    }

    fn collect_alternative(
        &self,
        participant: &Participant,
        alternative: &Alternative,
        request: &ResolveRequest<'_>,
        candidates: &mut Candidates,
    ) {
        let lines = self.route_lines(alternative, request);
        if lines.is_empty() {
            candidates
                .reasons
                .push("route has no seat for host harness".into());
        }
        for (seat, line) in lines {
            let Some(family) = self.family(&line.family) else {
                continue;
            };
            if !self.relation_matches(participant.relation, family, request) {
                candidates.reasons.push(format!(
                    "{} does not satisfy the author lineage relation",
                    line.id
                ));
                continue;
            }
            let harness = alternative.harness.as_deref().unwrap_or_else(|| {
                if family.harnesses.iter().any(|h| h == request.host_harness) {
                    request.host_harness
                } else {
                    &family.harnesses[0]
                }
            });
            let effort = if seat.is_none()
                && alternative.effort < Effort::High
                && self
                    .high_triggers
                    .iter()
                    .any(|t| request.trigger_facts.contains(t))
            {
                Effort::High
            } else {
                alternative.effort
            };
            for version in line.versions.iter().rev() {
                if seat.is_none()
                    && line.adoption == Adoption::Manual
                    && version.id != line.adopted_version
                {
                    continue;
                }
                if request.duty == "reasoning-support" && self.serves_host_seat(version, request) {
                    candidates
                        .reasons
                        .push(format!("{} is serving the seat", version.id));
                    continue;
                }
                let eligibility_request = EligibilityRequest {
                    duty: request.duty,
                    line: &line.id,
                    version: &version.id,
                    seat: seat.map(|s| s.id.as_str()),
                    harness,
                    effort,
                    exclusions: request.exclusions,
                    observed_ids: request.observed_ids,
                };
                self.consider_version(participant, line, version, &eligibility_request, candidates);
            }
        }
    }

    fn consider_version(
        &self,
        participant: &Participant,
        line: &ProductLine,
        version: &Version,
        request: &EligibilityRequest<'_>,
        candidates: &mut Candidates,
    ) {
        match self.eligible(request) {
            Ok(eligibility) => candidates.eligible.push(self.resolved(
                participant,
                line,
                version,
                request,
                eligibility,
            )),
            Err(reason) => {
                candidates.reasons.push(format!("{}: {reason}", version.id));
                if self.drift_candidate(line, version, request) {
                    let mut chosen =
                        self.resolved(participant, line, version, request, Eligibility::Candidate);
                    chosen
                        .pinned_id
                        .clone_from(&request.observed_ids[&version.pinned_id]);
                    chosen.requested_selector.clone_from(&chosen.pinned_id);
                    chosen.version.clone_from(&chosen.pinned_id);
                    chosen.limitations.push(
                        "identity drift: candidate only; designation and evidence discarded".into(),
                    );
                    candidates.drifted.push(chosen);
                }
            }
        }
    }

    fn drift_candidate(
        &self,
        line: &ProductLine,
        version: &Version,
        request: &EligibilityRequest<'_>,
    ) -> bool {
        let Some(observed) = request.observed_ids.get(&version.pinned_id) else {
            return false;
        };
        let Some(family) = self.family(&line.family) else {
            return false;
        };
        request.seat.is_none()
            && candidate_duty(request.duty)
            && version.lifecycle == Lifecycle::Active
            && observed != &version.pinned_id
            && !observed.trim().is_empty()
            && !self
                .lines
                .iter()
                .flat_map(|l| &l.versions)
                .any(|v| v.pinned_id == *observed && v.lifecycle != Lifecycle::Active)
            && Self::check_native(version, family, request).is_ok()
    }

    fn serves_host_seat(&self, version: &Version, request: &ResolveRequest<'_>) -> bool {
        let Some(seat) = self.host_seat(request.host_harness) else {
            return false;
        };
        let participant = Participant {
            id: "owning-seat".into(),
            alternatives: vec![Alternative {
                target: Target::Seat(seat.id.clone()),
                harness: None,
                effort: Effort::High,
            }],
            relation: LineageRelation::Any,
            label: ParticipantLabel::Required,
        };
        let seat_request = ResolveRequest {
            duty: "orchestrate",
            operator_override: None,
            requested_override: None,
            ..request.clone()
        };
        let mut result = Resolution::default();
        self.resolve_participant(&participant, &seat_request, &mut result);
        result
            .participants
            .first()
            .is_some_and(|chosen| chosen.version == version.id)
    }

    fn resolved(
        &self,
        participant: &Participant,
        line: &ProductLine,
        version: &Version,
        request: &EligibilityRequest<'_>,
        eligibility: Eligibility,
    ) -> ResolvedParticipant {
        let mut limitations = Vec::new();
        if !request.observed_ids.contains_key(&version.pinned_id) {
            limitations.push("native identity unobserved".into());
        }
        if eligibility == Eligibility::Designated {
            limitations.push("designated, full suite not run".into());
        }
        let reduced_assurance = request
            .seat
            .and_then(|id| self.seat(id))
            .is_some_and(|seat| seat.lines.first() != Some(&line.id));
        if reduced_assurance {
            limitations.push("seat fallback with reduced assurance".into());
        }
        ResolvedParticipant {
            participant: participant.id.clone(),
            label: participant.label,
            seat: request.seat.map(str::to_owned),
            line: line.id.clone(),
            version: version.id.clone(),
            lineage: self
                .family(&line.family)
                .map_or_else(String::new, |f| f.lineage.clone()),
            pinned_id: version.pinned_id.clone(),
            requested_selector: version.selectors[request.harness].clone(),
            harness: request.harness.into(),
            effort: request.effort,
            eligibility,
            reduced_assurance,
            limitations,
            remaining_alternatives: Vec::new(),
            operator_override: None,
            approval_owner: request.seat.map(str::to_owned),
        }
    }

    fn resolve_override(
        &self,
        participant: &Participant,
        request: &ResolveRequest<'_>,
    ) -> Result<ResolvedParticipant, String> {
        let record = request
            .operator_override
            .ok_or("missing OPERATOR_OVERRIDE record")?;
        if record.task != request.task {
            return Err("OPERATOR_OVERRIDE task mismatch".into());
        }
        if record.duty != request.duty || record.duty != "design" {
            return Err("OPERATOR_OVERRIDE duty mismatch".into());
        }
        nonempty(&record.plan_version, "OPERATOR_OVERRIDE plan version")?;
        nonempty(
            &record.instruction_record,
            "OPERATOR_OVERRIDE instruction record",
        )?;
        let route = request
            .requested_override
            .ok_or("missing OPERATOR_OVERRIDE invocation route")?;
        if route.target != record.route.target || route.harness != record.route.harness {
            return Err("OPERATOR_OVERRIDE route mismatch".into());
        }
        if route.effort != record.route.effort {
            return Err("OPERATOR_OVERRIDE effort mismatch".into());
        }
        if route.effort < floor("design", true) {
            return Err("OPERATOR_OVERRIDE effort below design duty floor".into());
        }
        let harness = route
            .harness
            .as_deref()
            .ok_or("OPERATOR_OVERRIDE needs exact harness")?;
        let (seat, lines): (_, Vec<_>) = match &route.target {
            Target::Seat(id) => {
                let seat = self.seat(id).ok_or("OPERATOR_OVERRIDE unknown seat")?;
                (
                    seat,
                    seat.lines.iter().filter_map(|id| self.line(id)).collect(),
                )
            }
            Target::Line(id) => {
                let seat = self
                    .seats
                    .iter()
                    .find(|s| s.lines.contains(id))
                    .ok_or("OPERATOR_OVERRIDE line is not a seat line")?;
                (
                    seat,
                    vec![self.line(id).ok_or("OPERATOR_OVERRIDE unknown line")?],
                )
            }
            Target::HostSeat => return Err("OPERATOR_OVERRIDE needs an exact seat or line".into()),
        };
        let mut reasons = Vec::new();
        for line in lines {
            for version in line.versions.iter().rev() {
                // The override changes the design line restriction only. Check
                // ordinary seat authority after explicitly checking the design floor.
                let eligibility_request = EligibilityRequest {
                    duty: "orchestrate",
                    line: &line.id,
                    version: &version.id,
                    seat: Some(&seat.id),
                    harness,
                    effort: route.effort,
                    exclusions: request.exclusions,
                    observed_ids: request.observed_ids,
                };
                match self.eligible(&eligibility_request) {
                    Ok(eligibility) => {
                        let mut chosen = self.resolved(
                            participant,
                            line,
                            version,
                            &eligibility_request,
                            eligibility,
                        );
                        chosen.operator_override = Some(record.clone());
                        chosen.limitations.push(
                            "OPERATOR_OVERRIDE applies to this task's design duty only".into(),
                        );
                        return Ok(chosen);
                    }
                    Err(reason) => reasons.push(format!("{}: {reason}", version.id)),
                }
            }
        }
        Err(format!(
            "OPERATOR_OVERRIDE route ineligible: {}",
            reasons.join("; ")
        ))
    }

    fn seat_version_worker(
        &self,
        primary: &ResolvedParticipant,
        participant: &Participant,
        request: &ResolveRequest<'_>,
    ) -> Option<ResolvedParticipant> {
        let line = self.line(&primary.line)?;
        let version = line.versions.iter().find(|v| v.id == primary.version)?;
        let tuple = EligibilityRequest {
            duty: "xhigh-reasoning",
            line: &line.id,
            version: &version.id,
            seat: None,
            harness: &primary.harness,
            effort: Effort::Xhigh,
            exclusions: request.exclusions,
            observed_ids: request.observed_ids,
        };
        let eligibility = self.eligible(&tuple).ok()?;
        Some(self.resolved(participant, line, version, &tuple, eligibility))
    }

    fn add_xhigh(&self, request: &ResolveRequest<'_>, result: &mut Resolution) {
        let seats: Vec<_> = result
            .participants
            .iter()
            .filter_map(|p| p.seat.as_ref())
            .cloned()
            .collect();
        let mut met = !seats.is_empty();
        for id in seats {
            let Some(seat) = self.seat(&id) else { continue };
            let participant = Participant {
                id: format!("xhigh-reasoning:{id}"),
                alternatives: self
                    .lines
                    .iter()
                    .filter(|line| line.family == seat.family)
                    .map(|line| Alternative {
                        target: Target::Line(line.id.clone()),
                        harness: None,
                        effort: Effort::Xhigh,
                    })
                    .collect(),
                relation: LineageRelation::Any,
                label: ParticipantLabel::Required,
            };
            let worker_request = ResolveRequest {
                duty: "xhigh-reasoning",
                requested_override: None,
                operator_override: None,
                ..request.clone()
            };
            let mut worker = Resolution::default();
            // A version already serving this seat may supply a separate xhigh
            // worker even when manual worker adoption still names an older one.
            if let Some(primary) = result
                .participants
                .iter()
                .find(|p| p.seat.as_deref() == Some(&id))
            {
                if let Some(chosen) =
                    self.seat_version_worker(primary, &participant, &worker_request)
                {
                    worker.participants.push(chosen);
                }
            }
            if worker.participants.is_empty() {
                self.resolve_participant(&participant, &worker_request, &mut worker);
            }
            if worker.participants.is_empty() {
                met = false;
                for open in &mut worker.open {
                    open.reasons.push(
                        "xhigh trigger not met; seat retains its entry effort and approval".into(),
                    );
                }
            }
            for chosen in &mut worker.participants {
                chosen.approval_owner = Some(id.clone());
            }
            result.obligations.extend(worker.participants);
            result.open.extend(worker.open);
        }
        result.xhigh_trigger_met = met;
    }
}
