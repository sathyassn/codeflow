---
id: ADR-0041
uid: 022b3ef1-05b6-466b-bc2e-61a6a576949a
title: select qualified model bindings by stable project roles
date: 2026-07-25
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — qualified model evidence gains a user-owned, reference-only project selection layer while the shipped ensemble remains fully managed
---

# ADR-0041 — role-based project model selection

## Context

ADR-0039 separated durable orchestration doctrine, harness capabilities,
qualified binding evidence, and the managed current ensemble. A consuming
project could qualify a different model but had no stable, project-owned way to
select it; editing `current-ensemble.json` conflicts with its fully managed
ownership. Durable instructions also named Fable where they meant the
responsibilities of the current Claude primary, making a later qualified model
promotion unnecessarily broad.

## Decision

Name stable primary roles in the managed ensemble and express durable duties in
those role terms. The current concrete bindings remain unchanged: Fable-high is
the Claude judgment primary and the strongest supported Sol-high model is the
Codex engineering primary until a controlled native evaluation and human
promotion decision replaces either.

Standard and full scaffolds seed `.codeflow/model-selection.json` as a
user-owned, reference-only file. Each entry maps one stable role and supported
harness to a binding ID already present under
`~/.codeflow/qualified-bindings/`; the harness comes from the qualified record,
so the project may select at most one binding per role/harness pair. It
contains no raw model selector, command, path, permission, or internal-worker
route.
`current-ensemble.json` stays fully managed and owns the defaults.

The deterministic resolver treats an absent or empty selection as the shipped
default. A present selection is atomic and fails closed when malformed,
duplicated, unknown, missing its record, ineligible for the exact role, mismatched
with the role's provider/lineage or supported harness, internally contradictory,
observably drifted, or collapsing the two primary lineages. An invalid selection
does not partially apply or silently fall back; doctor blocks orchestration
preflight until it is corrected or the project change is explicitly reverted.
Unprobeable live identity remains a named native-canary limitation rather than
an inferred pass.

Internal worker routing remains owned by the directly invoked primary and is not
project-overridable. CodeFlow validates and reports selection but does not
launch a harness, discover models, schedule work, or track vendor-internal
workers.

## Consequences

- A project can change one concrete primary through a reviewable one-line
  reference without copying fast-changing selectors through its doctrine.
- A repository edit cannot introduce an unqualified executable or arbitrary
  model string; it can only reference evidence the local operator already
  approved.
- Existing projects preserve current behavior. Projects that edited the managed
  ensemble move those choices into the new file after producing qualified
  records.
- Promotion and adoption remain separate. Full evaluation permits a binding;
  the project selection adopts it. Adoption uses an event-based probation and
  rolls back by reverting the selection entry.

## Rejected

- Put model choices in `.codeflow/policy.json`: model selection and git/security
  enforcement have different ownership and runtime concerns.
- Make the ensemble user-owned or managed-region: it weakens update convergence
  and permits free-form selectors.
- Automatic model discovery or promotion: native identity and judgment remain
  evaluation and human-approval concerns.
- Project overrides for internal workers: they couple CodeFlow to private
  harness scheduling without transferring primary responsibility.
