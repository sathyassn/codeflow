---
id: ADR-0056
title: High primary effort with bounded workers
date: 2026-09-07
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — development primaries default to high; worker effort remains task-dependent
---

# ADR-0056 — high primary effort with bounded workers

## Context

ADR-0055 set every development primary to medium. The operator has now approved
high as the primary default. The primary owns risk detection, task decomposition,
evidence interpretation, reconciliation, and acceptance—not merely dispatch.
Stronger workers cannot compensate for an unrecognized need to call them.

This is a quality-first policy decision, not a claim that high always wins.
[OpenAI GPT-5.6 guidance](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.6)
recommends a measured medium baseline; [Anthropic effort guidance](https://platform.claude.com/docs/en/build-with-claude/effort)
recommends high for current Fable/Opus with evaluated step-downs. Effort scales
are model-specific. The local medium diagnostics found real routing failures
but lack a matched high control; their historical results remain unchanged.

## Decision

Current effort authority is this decision and its projection in
[`docs/architecture.md`](../architecture.md). Earlier decisions are historical:

| Earlier record | Superseded portion | Retained portion |
|---|---|---|
| [ADR-0054](ADR-0054-grok-first-class-host-and-catalog.md) | Later medium-default effort note | Grok host/catalog and extra-family duties |
| [ADR-0055](ADR-0055-medium-default-astra-contained-worktrees.md) | Medium primary default and its medium-primary escalation wording | Selectors, permissions, transport, worktrees, and Herdr rules |

This explicit partial supersession preserves the historical records unchanged.

For clarity when reading ADR-0055, Codex's approval policy alone does not grant
access: the separate sandbox setting controls access. Its production launch and
non-bypass consult/review distinction remain unchanged; an approval flag is not
an interchangeable permission or sandbox bypass.

- Supersede only ADR-0055's primary-effort default: Claude, Codex, and catalog
  Grok development primaries enter at high. Keep model selectors, permissions,
  transport, worktree policy, and independent cross-family duties unchanged.
- The receiving primary owns its workers. Cross-family callers enter the
  qualified primary at its default effort as a peer; they never invoke a
  foreign worker or pass an xhigh worker's effort on the primary entry.
- Medium remains appropriate for bounded routine workers. Use the strongest
  qualified relevant reasoning seat for consequential work. A high primary may
  do suitable reasoning directly; an additional high worker must earn its cost
  through specialization, independent scrutiny, or useful parallelism.
- When an xhigh trigger applies, obtain xhigh reasoning through a supported
  same-family worker route without manufacturing a failed high attempt. The
  primary stays accountable for integration and approval. Unknown native
  capabilities or results remain explicit gaps, not assumed compliance.
- Codex project settings pin high. Claude settings provide `effortLevel: high`
  as a starting default; explicit launches still pass the ensemble effort.
  Model-specific/user/environment overrides must be checked, not overwritten
  globally. Do not use a global effort environment variable: it can suppress
  child-specific effort. Routine persistent-assistant operation outside the
  development workflow is not changed by this decision.

## Verification and consequences

Sync canonical assets, installed mirrors, baselines, and manifests. Test primary
defaults separately from permitted medium workers, actual cross-family entry,
xhigh triggers, routine restraint, and missing native worker evidence. Record
new native trials against their exact suite/revision; do not relabel earlier
medium trials or claim a full binding promotion from focused diagnostics.

High increases baseline resource use. Evaluate total cost including workers,
verification and rework, alongside missed escalation and final quality. Higher
effort never relaxes scope discipline, proportionate code, or independent review.
