---
name: cf-estimate
description: Provides an optional agentic operating and estimation method for software delivery using evidence-anchored Easy/Medium/Hard/Very Hard grades, populated scenario estimates and feasible resource allocations. Use on an explicit request for agentic delivery estimates, forecasts, capacity, deadlines, or revising estimates from outcomes; a project-specific preview comes before confirmed adoption. Do not use for personal-task scoring, model-effort selection, automatic scheduling, or implementation.
---

# cf-estimate — estimate agent-delivered outcomes

Produce a useful, evidence-labelled forecast, not a human staffing estimate with
an AI speed multiplier. This method is a starting point a project may adopt,
adapt or decline. It does not replace its task authority, engineering standards,
independent review, security controls or acceptance criteria.

Use inside the active `cf-model-orchestrator` flow; direct use for routed work
first enters that flow. Both primary seats independently assess material scope,
drivers, duration assumptions and feasibility, then reconcile from evidence.
Do not restart an already settled interview or choose model effort from a grade.
An unavailable peer follows the existing evidenced-degradation contract.

## Establish the project and the decision

Read its purpose, current scope and acceptance, architecture, work authority,
delivery state, existing estimation method and comparable outcomes. Inspect
`.codeflow/estimate.json` if present and the installed checker capability. A
missing tool is a disclosed limitation, not permission to install it. Identify
the decision: explore an operating model, estimate work, compare a deadline or
capacity option, revise a forecast, or assess actual outcomes.

- **Not adopted:** answer the request, and with it offer a project-specific
  preview: a representative package, populated scenarios and what constrains
  delivery. Explain the benefit and cost relative to existing practice. Never
  offer it unasked in other work. Ask before adopting it or creating durable
  project records.
  This includes project-owned drafts labelled proposed. Keep an unadopted
  preview in the response or task-owned temporary scratch; a re-offer event
  does not grant adoption.
- **Adopted:** reuse the compatible profile, rubric version, authority and
  record home. Ask only for consequential missing facts or changed constraints,
  not the same setup questions on every request.
- **Declined:** honor the recorded decision until its material re-offer event
  occurs. A direct request may still be answered; it is not automatic adoption.
- **Existing external method/tracker:** preserve its authority. Offer a linked
  forecast or comparison only where useful; never clone task status, acceptance
  or dependencies into a competing tracker or local database.

An estimate request authorizes an answer, not execution, adoption, installation,
tracker replacement, risky probes, commitments to outsiders or relaxed gates.
The operator supplies genuinely owner-held facts—priorities, budget, authority,
availability—not all estimates. Resolve discoverable facts yourself.

## Apply the method

1. Establish a pinned delivery package. Anchored grades are optional: where
   they help, read [rubric.md](references/rubric.md) to assign anchored
   drivers. Materially missing scope/ownership/acceptance is
   UNSIZED; duration uncertainty alone is not. Show conditional alternatives
   separately. Keep grade, consequence, permission and model effort distinct.
2. Read [operating-model.md](references/operating-model.md) to estimate the full
   delivery boundary from comparable evidence, a safe authorized probe, or
   explicit judgment. Return favorable/planning/adverse scenarios with actual
   durations and reasons, not empty templates or universal grade-to-hour tables.
3. Allocate independent work against actual resources, dependencies, review and
   human/release availability. Check a feasible scenario, not just a capacity
   lower bound. Preserve sensitivity and unresolved disagreement.
4. For durable adoption/snapshots or checker use, read
   [records.md](references/records.md). The optional native
   `codeflow estimate check <forecast-path> --json` verifies supplied data and
   allocations; it does not grade, predict, schedule, approve or execute work.
   Repair findings in the proposed snapshot; never rewrite an old prediction.
5. For a first application, use [worked-example.md](references/worked-example.md)
   to understand the calculation and adapt to this project's evidence. Its
   fictional numbers are examples, never starter productivity coefficients.
6. At material change or closeout, preserve the original forecast and every
   started outcome. Revise with a reason and predecessor link. Follow the
   prospective comparison procedure in the operating reference before making
   any claim of local predictive usefulness.

## Return an actionable result

Lead with the forecast or decision and its basis. Show package/category or
milestone estimates, grade anchors where graded, full-delivery boundary, resource consumption
separately from active elapsed and calendar lead time, limiting dependencies,
assumptions, human decisions, sensitivity and the next useful action. Use
project-defined sessions only with a stated duration/boundary; use days or months
when the actual agentic schedule warrants them, not human-development habit.
Scenario labels are not probabilities or statistical confidence intervals.

State what was checked, what remains judgment, missing evidence and any reduced
assurance. Write the report plainly: simple, straightforward and clear, no
mannered prose (see `.codeflow/rules/writing.md`); use project editorial
guidance and `cf-editorial-review` for a substantial one. Do not turn the
report into a mandatory ceremony or create an epic merely to estimate one
task. Implementation starts only through the existing approved work
lifecycle, with unchanged quality and authority gates.
