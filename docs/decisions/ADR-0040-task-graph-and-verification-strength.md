---
id: ADR-0040
uid: f497ce8a-be43-4a6e-93c4-8913d8beacc9
title: settle task graphs and select verification strength by evidence
date: 2026-07-25
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — multi-task plans gain a non-executable task graph with structurally validated task dependencies, and verification planning gains evidence-triggered property, mutation, and architecture-fitness options
---

# ADR-0040 — task graphs and verification strength

## Context

The plan contract named assignments and free-form dependencies but did not
define one reviewable graph or the boundary between ordinary execution detail
and a material plan mutation. Task records also carried ad hoc dependency
metadata that the model, template, and documentation validator did not own.
Separately, normal example, integration, end-to-end, and coverage checks did not
say when deeper property, mutation, or architecture-fitness evidence was worth
its cost.

## Decision

For multi-task work, both primary seats approve one acyclic Plan vN task graph.
Bare edges express active finish-before-start dependencies; observable guards
are reserved for genuine pre-approved decisions. Every task records its direct
structural candidate predecessors in canonical `depends_on`; the legacy
`dependencies` spelling remains read-compatible, and defining both is invalid.
The metadata is not executable: Plan guards and ledger evidence select branches,
record unselected alternatives, and determine join readiness.
`codeflow validate --docs` checks task identity, filenames, references,
duplicates, self-edges, and cycles, but never schedules work or interprets
guards. A material node, edge, guard, interface, ownership, acceptance, or
safety change creates Plan vN+1; bounded implementation detail inside an
approved node does not.

Verification planning may additionally select:

- property or generative tests when a stable invariant and meaningful input
  space exist, failures are reproducible and shrinkable, and examples could
  miss material combinations;
- targeted, time-bounded mutation testing for material decision, state,
  security, recovery, or guard logic after the base suite is reliable; and
- a project-owned architecture fitness check when a current invariant has a
  deterministic observable rule and a decision or recurrence reason to protect
  it.

These techniques complement singular regression examples and ordinary tests.
CodeFlow does not install them universally or add them for tool parity;
`cf-stack` and `cf-customize` use only the consuming project's justified,
reviewed commands. Heavier ownership, cadence, exception, and retirement
process is proportional to heavyweight or organization-level gates, not a
prerequisite for every cheap in-suite invariant.

## Consequences

- Plans expose the real critical path, decision points, safe parallel
  eligibility, and integration order without coupling CodeFlow to a harness
  task UI.
- Existing task records remain valid. Typed rewrites normalize the legacy
  spelling; malformed or cyclic graphs fail with repository-local diagnostics.
- Guarded joins list all structural candidate predecessors. Only active
  predecessors must land; alternatives resolve explicitly rather than
  pretending to complete.
- Stronger verification is justified by risk and oracle quality. A passing
  percentage, mutation score, or fashionable tool is never the objective.
- Graph and verification-detail resources remain progressive disclosure, so
  single-task or ordinary changes do not pay their full prompt cost.

## Rejected

- Prose-only dependencies: too easy for plan, task records, and review evidence
  to diverge.
- A CLI scheduler or runtime DAG engine: duplicates native harness mechanics
  and would require executable guard/readiness semantics that CodeFlow does not
  need.
- Fixed workflow packs or mandatory property/mutation/fitness tooling: too
  prescriptive across consuming stacks and disproportionate for ordinary work.

## Architecture impact

The records validator owns structural task dependency integrity. The scaffolded
orchestrator owns Plan graph settlement, mutation boundaries, and
evidence-selected verification guidance; neither surface executes the graph.

## Note, 2026-09-29: ADR-0076 narrows one clause

ADR-0076 (one PR per task and planning once per epic) is accepted. It
narrows one clause of this record and no other.

- "A material node, edge, guard, interface, ownership, acceptance, or
  safety change creates Plan vN+1". Narrowed: Plan vN+1 is needed for a
  material change of outcome, cross-task interface, dependency graph or
  safety boundary. An ownership change rides in the batched epic
  amendment, and a task's own criteria change rides in its PR, where CI
  prints it for the reviewer (SPC-013 R-52, R-74). The graph validation and
  the evidence-selected verification options are unchanged.
