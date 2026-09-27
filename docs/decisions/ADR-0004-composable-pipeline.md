---
id: ADR-0004
uid: 8de6918f-ec9a-4907-9319-945289a76838
title: Composable pipeline workflow, shipped user-owned
date: 2026-06-12
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0004 — Composable pipeline workflow, shipped user-owned

## Context

The one shipped workflow (`develop.workflow.js`) was a fixed
build -> review -> verify sequence, update-managed like the other Claude
artifacts. Real use needs more shapes — analysis or planning up front, a
security or QA pass, per-stage model routing, unattended batch runs — and a
managed sequence is a rot liability: the METR-style evidence, the charter's
D6 deletion of orchestration organs, and the 90-day humility rule all point
the same way — sequencing conventions dissolve fast, verification survives
them. The mechanics a composable shape needs (stages as data, a per-stage
model option, schema verdicts, bounded rework) were validated live in run
wf_74f22cff-d1a: a deliberately sabotaged build was caught by the schema
verdict, the rework back-edge fired, and the run recovered in 2 attempts.

## Decision

Ship one composable pipeline workflow,
`.claude/workflows/pipeline.workflow.js`, as a stages-as-data reference —
user-owned, not managed: seeded once at init and never touched by
`codeflow update`; consumers own and adapt their copy. Stages (analyze,
plan, build, security, review, qa, verify) are rows in a data table. Model
routing and presets are configuration, not code: `args.stages` (default
`['build', 'review', 'verify']`), `args.models` (default `'inherit'` per
stage), optionally fed from a `[workflows]` table in
`.codeflow/project.toml`. Review-class gates return the schema verdict
`{verdict: approved|changes_requested, findings[]}` and back-edge to build
under one shared bounded budget that throws loudly on exhaustion. The
interactive default stays /cf-develop's inline loop; the pipeline is for
unattended or batch runs and explicit invocation.

## Consequences

- `develop.workflow.js` is deleted; `pipeline.workflow.js` replaces it in
  the scaffold manifest with ownership `user-owned` (written only if
  absent).
- Consumer copies drift from the shipped reference by design; fixes to the
  reference reach existing projects only by deliberate adoption, never by
  update. That is the point: no framework contract to rot under them.
- Sequencing stays cheap to rewrite (the harness-coupled artifact class);
  the durable contract is verification — gates, schema verdicts, codeflow
  exit codes.
- Presets are defaults, not constraints: ad-hoc workflows are sanctioned
  whenever the work fits no preset.

## Architecture impact

None — scaffold asset content and an ownership-class change only; no
shipped module boundary moves.
