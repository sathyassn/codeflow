---
id: ADR-0039
uid: d3d8ad7d-0649-4d16-8c0d-6528f60d120b
title: separate durable orchestration doctrine from qualified model and harness bindings
date: 2026-07-25
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — model qualification gains a universal harness capability catalog, one current ensemble record, composable diagnostic packs, promoted local binding records, and doctor drift checks without becoming a runtime router
---

# ADR-0039 — qualified model and harness bindings

## Context

CodeFlow's orchestration duties should survive model releases, but concrete
model selectors, effort defaults, worker classes, and native harness behavior
change quickly. Repeating those bindings across skills and harness notes makes
updates inconsistent. Accepting any new harness string, on the other hand,
would confuse availability with proof that the harness supplies native
provenance, tools, permission boundaries, bounded failure, and recheckable
results.

The evaluation kit already treats model, effort, harness, settings, tools, and
network as one evaluated system. It lacked a compact promotion artifact and a
way to report when externally observable parts of that artifact drifted.

## Decision

Keep four explicit layers:

1. durable doctrine in `cf-model-orchestrator` and its quality/routing
   contracts;
2. a source-controlled harness capability catalog whose
   `capability-supported` entries satisfy the same minimum native contract but
   do not, by themselves, qualify a model binding;
3. user-owned promoted binding records derived only from approved full
   native-interactive evaluation results; and
4. one current ensemble record owning concrete primary selectors, effort
   policy, internal worker classes, and escalation triggers.

`record-binding` rejects incomplete runs, hard-case failures, validity flags,
missing human approval, and requested/observed model or effort mismatch. It
writes non-secret metadata and digests under
`~/.codeflow/qualified-bindings/`; it never copies prompts, settings contents,
credentials, or trace bodies.

`codeflow doctor --check model-bindings` validates record structure against the
trusted harness catalog. It compares installed harness versions when the
catalog names a code-allowlisted probe ID and recomputes any declared
settings-source digests. The catalog cannot provide commands or arguments.
With no optional local record, the check is not applicable and passes without
claiming a binding; an externally unobservable version is a warning. Malformed
or internally contradictory claimed evidence fails. Doctor never launches a
model or infers the live model, effort, route, quota, or availability.

The evaluation corpus also gains acyclic diagnostic packs. Packs compose
existing cases for focused work; they do not change graders or qualify a
binding. Promotion still requires the complete full suite.

## Consequences

- Model changes update the ensemble/binding surface; harness changes update
  the capability catalog. Either change then proves the concrete binding
  through native full-suite evaluation.
- A future provider can be added without a generic plugin engine, but only
  after its real harness meets every catalog capability. That
  capability-supported status is not model qualification: each concrete
  binding still needs an approved full native result. A live version probe
  additionally requires a small reviewed code allowlist change; a catalog edit
  cannot make CodeFlow execute a new command.
- Local records make requested/observed and settings/version drift legible
  without turning CodeFlow into a launcher, scheduler, or automatic router.
- Codex App has no trusted external version probe, so its record remains a
  warning until a native canary establishes freshness. This is more honest than
  a guessed pass.
- Existing independent Claude+Codex planning, Claude-led design, per-task
  producer assignment, cross-lineage review, integrated Claude judgment,
  safety boundaries, and graceful degradation are unchanged.

## Rejected

- A generic provider plugin API or executable registry: no second independent
  integration yet justifies runtime machinery or arbitrary commands.
- Automatic model discovery, promotion, or routing: native surfaces do not
  expose enough uniform evidence, and human approval remains required.
- Tracking vendor-internal subagent IDs: internal scheduling is harness-owned
  and would create brittle coupling.
- Predeclaring untested Grok, Kimi, ACP, or A2A bindings: protocol presence does
  not meet the native capability contract.

## Note (2026-09-25)

Layer 4 is now the schema 5 model catalog (ADR-0069): families, product lines
with ordered versions, seats and duties, still one fully managed
`current-ensemble.json`. It owns pinned ids, efforts and triggers in place of
primary selectors and internal worker classes. A designation is not a
promoted binding: a seat served by designation alone is reported as
"designated, full suite not run", and qualification still needs the full
suite and an approved binding record under this decision. `doctor --check
model-bindings` adds illustrative catalog resolutions and keeps the
harness-version and settings drift checks for binding records.
