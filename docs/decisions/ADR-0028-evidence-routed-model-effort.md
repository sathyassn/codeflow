---
id: ADR-0028
title: route model effort by evidence and task demand
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — native duo seats use high by default, xhigh on explicit complexity or failure triggers, and bounded native workers without transferring primary ownership
---

# ADR-0028: evidence-routed model effort

## Context

ADR-0024 fixed both primary duo seats at xhigh. That maximizes per-turn effort,
but treats every non-trivial task as equally demanding and spends extra latency,
quota, and context without evidence that every task benefits. The independent
planning, dual approval, fixed implementation/review roles, and quality gates
are semantic duties; reasoning effort is a qualified runtime binding, not a
substitute for those duties.

## Decision

Amend the effort selection in ADR-0024 without changing any duo role or stage.
Cross-model callers invoke the latest Fable-class Claude seat directly at high
by default and escalate that primary to xhigh for capability-sensitive or
long-horizon work, material ambiguity, cross-cutting architecture/security,
unresolved duo disagreement, or a failed/stalled high run. Fable owns its native
session and may route bounded deterministic browser/UI/MCP evidence collection
to current Opus-class workers at medium, or ambiguous/multi-step tool operation
at high; Fable interprets their evidence and owns every judgment.

Callers invoke GPT-5.6 Sol or the strongest supported successor Codex coding
seat directly at high by default and escalate to xhigh on the same triggers.
The trusted project config pins `model_reasoning_effort = "high"` as the
ordinary fallback. A Claude-hosted official-plugin task passes `--effort high`
or `--effort xhigh` explicitly so the observed turn cannot inherit an unrelated
user default.
Codex may use bounded Sol-class medium workers for localized deterministic work
and high workers for complex independent work only through verified native
per-worker routing. The primary Codex seat retains implementation and first
verification.
An internal worker never replaces a primary duo seat, its independent plan, or
its approval.

Record the selected model, effort, escalation reason, and observed native
binding. A different default or trigger set must pass the controlled,
one-variable native-interactive qualification in ADR-0027; lower cost or latency
cannot compensate for a semantic or quality regression.

## Consequences

- Routine non-trivial work uses strong reasoning without paying the xhigh cost
  automatically; demanding or stalled work has an explicit escalation path.
- Harnesses invoke the peer's primary reasoning/coding seat, while each peer
  owns any internal worker routing its native harness can actually prove.
- Worker specialization can reduce mechanical cost without transferring design,
  implementation, verification, or review accountability.
- The evaluator must test both selection behavior and observed high/xhigh
  bindings before this policy is changed again.

## Architecture impact

The orchestrator, consult/delegate adapters, onboarding guidance, always-loaded
Claude guidance, capability record, and behavioral evaluation corpus share one
high-default/xhigh-trigger policy. The binary remains model agnostic.

## Note (2026-09-05)

ADR-0055 amends the default: primaries start at medium and escalate to high
then xhigh on recorded complexity/difficulty triggers. The rest of this
decision (primary-owned internals, no worker replacing a primary, qualification
before a binding change) stands.
