---
id: ADR-0038
title: preserve critical-path focus without reflexive deferral or escalation
date: 2026-07-24
status: accepted
superseded_by: null
architecture_impact: workflow, duo quality, reviewer, develop-loop, and model-evaluation artifacts share one execution-focus, deferral, and blocker-navigation rule
---

# ADR-0038 — critical-path focus and deferral stewardship

## Context

ADR-0034 made review and proactive discovery consequence-led. It did not fully
specify how a model should allocate attention during execution. Two bad
interpretations remained possible:

- doing quick cosmetic work while an accepted material blocker remains
  unresolved; or
- using “critical path” as a reason to postpone every secondary improvement,
  including clear, safe, in-scope work that is cheaper to verify while the
  relevant context is already loaded.

Per-observation peer consultation or issue creation would preserve information
at the cost of interruption, noise, and planning debt. Time-based reminders
would add machinery without describing the event that makes the work relevant.
Existing bounded-loop wording also risked escalating mechanically after a
retry count, even where a different evidenced hypothesis or reversible
strategy could preserve the accepted outcome.

## Decision

Materiality is the consequence of an issue or opportunity. The current critical
path is the dependency or blocker presently controlling the accepted outcome.
Models keep both explicit and reassess them when evidence, dependencies, or
gates change.

Critical-path focus never relaxes the quality contract: accepted correctness,
security, testing, independent review, documentation, recovery, and delivery
evidence remain required. Allied work belongs on the active path when it
unblocks or de-risks the outcome. A clear, safe, in-scope improvement also
belongs in the current task when its focused validation is bounded; models
should normally fix it while context is warm rather than manufacture backlog.

Only genuinely uncertain secondary observations are consolidated. At the next
natural integrated review or closeout checkpoint, both available primary
lineages inspect that batch and choose exactly one disposition per item:

1. fix now;
2. track once at an existing repository planning altitude; or
3. drop as non-actionable.

A retained item records evidence, expected value, and a deterministic revisit
event such as the next touch of the affected surface, a named dependency
landing, a named release or quality gate, or recurrence of the symptom.
Calendar age, “someday,” and “later” are not revisit triggers. Missing peer
availability is recorded as reduced assurance; it does not permit claiming
dual review. Dispatch is not disposition: the host cannot close the checkpoint
from a background launch, notification promise, relay idle signal, or transport
completion. It waits for the bounded result and verifies native provenance and
content.

No new CLI surface, skill, generic tracker, scoring framework, or automatic
issue creator is introduced.

An impediment is classified before escalation:

- discoverable facts and technical failures are reproduced and isolated, then
  receive a bounded probe tied to a new hypothesis;
- local reversible implementation choices inside accepted intent follow
  repository evidence and are disclosed;
- external dependencies and enforced gates name the exact evidence or input
  that clears them; and
- only choices that change desired outcome, public contract, scope/authority,
  risk tolerance, or an irreversible tradeoff are operator decisions.

An earlier failure may be confirmed once when freshness or provenance
materially affects the decision, and the confirmation states its evidence
question. Once the failure is current, the same attempt is not repeated again:
the model changes hypothesis or strategy. After a bounded tactical cycle, the
model restates the real constraint and critical path, compares viable
strategies, and may reroute only while preserving accepted outcome, scope,
authority, and every quality/safety gate. Operator escalation includes verified
state, attempts, options and consequences, and a recommendation. A red
deterministic or safety gate must be fixed or honored; its color alone does not
transfer an implementation choice to the operator.

## Note (2026-09-05)

Classify gate redness before honoring or escalating it. A **gate** is the
verification check, not the CI job. Assertion-red (the check completed and
failed) is fixed or honored; model consensus cannot override it. A job that
never finished (runner, memory, billing, timeout with no result) is
infra-incomplete: missing job evidence. A completed same-check in a sibling
job or local `codeflow test` satisfies that gate. Asking the operator to
choose an implementation tactic because a job name is red remains forbidden;
waiting, rerunning, or overriding a host required-status that is
infra-incomplete is merge authorization, not a failed test.

## Consequences

- Models direct capable attention and tools to the outcome-controlling path
  without treating quality work as optional.
- Small but earned improvements can be completed promptly instead of being
  reflexively deferred.
- Uncertain observations survive in one deliberate batch without per-nit
  interruption or issue farming.
- Revisit conditions are testable events rather than stale time labels.
- Technical blocks either gain evidence or change strategy; retry loops cannot
  masquerade as persistence.
- Operator attention is reserved for real dependencies and owner decisions
  without authorizing models to invent product intent.
- Native model evaluations can grade the subject's repository actions and
  proposed disposition batch. Actual two-family checkpoint evidence remains a
  separate orchestration requirement and is never inferred from a
  single-subject trial.

## Alternatives considered

### Defer every non-critical observation

Rejected because it creates avoidable backlog and discards the advantage of
warm context.

### Fix every observation immediately

Rejected because preferences and uncertain low-value work can interrupt the
accepted outcome and consume review capacity.

### Add a dedicated backlog or cleanup skill

Rejected because existing project/task planning altitudes are sufficient and a
new mechanism would encourage duplicate state.

### Use age-based cleanup

Rejected because elapsed time does not identify when an observation becomes
relevant or actionable.
