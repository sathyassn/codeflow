---
id: ADR-0034
title: prioritize substantiated material findings without issue farming
date: 2026-07-20
status: accepted
superseded_by: null
architecture_impact: the always-loaded workflow principle, duo quality contract, reviewer adapters, and model evaluator share one materiality and proactive-routing rule
---

# ADR-0034 — materiality and proactive stewardship

## Context

CodeFlow already requires broad research, evidence, independent review,
proportionate design, and safe scope control. A capable model can nevertheless
spend its attention on easy naming or formatting changes while a consequential
correctness, security, structural, or operational concern receives less weight.
The opposite correction—suppressing every small observation—would hide useful
polish and miss repeated symptoms of one systemic defect. Proactive discovery
also needs a boundary: noticing material risk must not become silent scope
expansion, external mutation, or issue generation for its own sake.

## Decision

Adopt one cross-domain sequence: find broadly, substantiate, classify,
prioritize, and route. Severity describes the consequence if unresolved;
confidence describes evidence strength. Priority combines those with likelihood
or reachability, blast radius, urgency or cost of delay, recurrence or systemic
leverage, and dependencies. Remediation effort informs sequencing and ownership
but never relabels severity.

General review keeps the existing `blocker | major | minor` vocabulary.
Blocker and major findings lead. Cosmetics, stylistic preferences, and nits are
minor and non-blocking; a review approves when they are the only findings and
still lists them after the evidence. Repeated minor-looking symptoms are
investigated for one systemic cause. Security review keeps its CVSS-aligned
severity and separate confidence rather than inheriting the general vocabulary;
a general gate treats a confirmed or likely critical/high security finding as a
blocker.

For out-of-scope discoveries, an evidenced imminent severe risk is escalated
immediately. Another material observation is routed to one tracked item with
evidence and a proposed next step. Isolated nits are noted or batched, not
converted into one issue each. Discovery never grants edit, scope-expansion, or
external-mutation authority. “Nothing material found” is a valid result.

Encode the rule once in the shared quality contract, carry a compact principle
in both scaffold tiers, align the reviewer adapters, and add the hard
`CF-QA-005` requirement with paired regression cases. Do not add a new runtime,
review schema, general cleanup skill, or cosmetic CI gate.

## Consequences

- Review attention follows consequence and evidence rather than ease of repair.
- Minor polish remains visible without delaying delivery or crowding the report.
- Proactive discovery can surface material risk while preserving task authority
  and avoiding issue farming.
- Security retains a standards-aligned severity model instead of being flattened
  into general review labels.
- Model bindings can be tested for both under-prioritization and over-triggering.

## Sources

- Google engineering review standard:
  <https://google.github.io/eng-practices/review/reviewer/standard.html>
- GitLab code review guidelines:
  <https://docs.gitlab.com/development/code_review/>
- Git reviewing guidelines:
  <https://git-scm.com/docs/ReviewingGuidelines.html>
- OWASP risk rating methodology:
  <https://owasp.org/www-community/OWASP_Risk_Rating_Methodology>
- Google SRE risk analysis:
  <https://cloud.google.com/blog/products/devops-sre/how-sres-analyze-risks-to-evaluate-slos>
