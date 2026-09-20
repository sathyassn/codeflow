---
id: ADR-0057
title: Optional agentic operating and estimation method
date: 2026-09-09
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — optional linked forecasts and read-only allocation checking
---

# ADR-0057: optional agentic operating and estimation method

## Context

CodeFlow already provides planning, independent model-family review, bounded
parallel execution, verification and durable work records. It does not yet
offer a complete method for estimating accepted agent-delivered outcomes from
project evidence. Human developer-days and vendor speed multipliers are not a
substitute for measuring implementation, review, QA, rework, shared resources
and human decision/release availability.

The operator approved adapting the local agentic-development-model corpus at
commit `87852d69170fad6b15bf2463841c74ef80ceda58`, with explicit repairs and
prospective evaluation. The original remains historical input. Its assumed
hour bands and arbitrary qualification sample counts are not adopted defaults.

## Decision

Offer a complete optional `cf-estimate` method at planning/customization and
ordinary estimation or agentic operating requests. Inspect existing project
context and authority first; provide a useful comparison, confirm adoption,
reuse an adopted compatible profile, and respect a recorded decline until a
material event changes the need. An estimate request does not authorize
implementation, tool installation, tracker replacement or risky probes.

Retain a versioned evidence-anchored Easy/Medium/Hard/Very Hard rubric with
repaired ownership and semantic refusal counting. Connect grades to local
comparable ranges only where justified; supply explicit judgment estimates
when evidence is absent. Retain uncertainty, full-delivery boundaries, every
started outcome, original predictions and their revisions. Grade is neither
model effort nor permission, and no grade lowers existing engineering gates.

The native CLI gains one narrow verb, `estimate check`: validate explicit
forecast allocations, source pins, references, resources and arithmetic. Agents
construct the scenarios; the helper does not schedule or optimize. It never
approves quality or execution, changes task status, invokes models, fetches
sources, or writes adoption/work records. This is data verification, not a
process-enforcement engine. Future verbs need a new justified decision.

Use strict typed JSON for calculation snapshots and independently versioned
machine-readable output. Existing Markdown work records remain authoritative;
forecasts link them rather than copy status, acceptance or package dependencies.
Epoch UTC seconds plus bounded integer offsets are deliberate calculation units,
distinct from human-readable RFC3339 event timestamps elsewhere. Avoid a new
time/date dependency and implicit time-zone or calendar assumptions. Whole-file
pins include Markdown acceptance; a harmless edit can invalidate a current
check, but never causes silent historical rewriting.

Project-owned `.codeflow/estimate.json` records adoption/decline and the chosen
record home. It is absent from the scaffold manifest. The skill owns optional
profile/snapshot creation after confirmation, not the checker or `init/update`.
Standard/external-tracker projects retain their authority and receive explicitly
limited source assurance; no synthetic CodeFlow work graph is created. Agent OS
routes software work to the installed method through its existing native-host
handoff, not a second estimator or inner task system.

## Alternatives

- **Python-only skill helper:** a viable general pattern, but Python is not a
  universal CodeFlow installation prerequisite and standard-library Python
  cannot reuse canonical YAML/legacy task readers. Avoid a second parser.
- **Only written guidance:** useful but insufficient for repeatable arithmetic,
  pin, graph and capacity failures in a quantitative method.
- **Automatic scheduler or statistical platform:** adds assumptions, runtime and
  maintenance beyond the current need. Explicit allocation checking earns its
  smaller scope. A capacity lower bound is not a feasible schedule.
- **Another `validate` flag:** possible, but a named estimation check keeps its
  input/output and failure domain clear. Discoverability still comes primarily
  from skill routing, not expecting users to find a CLI verb.

## Evidence and limits

Structured judgment is legitimate when data are sparse, but model agreement is
not statistical confidence. Environment-specific size/time relationships and
resource calendars matter. These methodological influences do not validate the
candidate rubric's thresholds or predict this project's speed:

- [SHELF elicitation framework](https://shelf.sites.sheffield.ac.uk/).
- [COSMIC estimating with software size](https://cosmic-sizing.org/cosmic-sizing/estimating-with-software-size/).
- [GAO Schedule Assessment Guide](https://www.gao.gov/assets/gao-16-89g.pdf).
- [NIST censored observations](https://www.itl.nist.gov/div898/handbook/apr/section1/apr131.htm).
- [Forecast distribution accuracy](https://otexts.com/fpp3/distaccuracy.html).

SPC-007 and EPC-006 Plan v2 separate deterministic correctness, native behavior,
installed workflow usability and prospective predictive usefulness. Tests and
a small pilot may establish implementation readiness, not empirical calibration.
An added grouping/rubric complexity that fails to beat a simpler local baseline
must not be defended as predictive value without evidence.

## Consequences

The method is actively offered but optional, with no install-time profile or
calibration table. Consumers gain a usable starting method and explicit limits,
not a delivery guarantee. Stable JSON and CLI surfaces add compatibility duties;
closed schemas, bounded input, cross-platform tests, golden reports and
independent review carry those duties. Source/task authority remains singular.
