# Operating and estimating agent-delivered work

Use this after project intake and rubric scoring. Agents perform implementation
and verification; people retain product direction, consequential decisions and
the project's approval authority. Automation changes execution capacity, not
those responsibilities or the definition of done.

## Delivery boundary

Use the existing discussion → clarified intent → approved work → implementation
→ independent review → verification → integration/release lifecycle. CodeFlow's
orchestrator and workgraph own those rules. Estimation reads and links them; it
does not create a competing state machine, scheduler or work tracker.

Estimate through required independent review, verification and expected in-scope
rework, not merely the first patch. Name integration, human decisions and release
activities in the whole forecast. They need not be charged to every package.
Consumed APIs/platforms may score zero as owned surfaces but still require
integration, failure handling, testing and environment effort. Reused components
are not free; avoid double-charging common foundation work across packages.

Work that cannot merge independently may share a co-delivery cluster, with each
package retaining its own scope/grade and one explicit integration activity.
Resolve the union of their real dependencies; do not invent a max-band shortcut.
Use the existing integration-branch doctrine for coherent batches. Parallelism
is conditional on file ownership, host memory, tools, environments and reviewers,
not the number of available agent names.

## Produce durations even at cold start

Select and disclose a basis for each estimate:

1. **Comparable evidence:** use full-delivery actuals from sufficiently similar
   work, environment, model/tool capability and acceptance boundary. Explain
   mismatches and exceptions; a coding-only benchmark is not full delivery.
2. **Representative probe:** if permitted, time a bounded disposable experiment
   exercising the important uncertainty. Obtain authority before risky or
   external actions. State the partial boundary and estimate omitted stages;
   never label a small probe a complete delivery observation.
3. **Explicit judgment:** when data or safe probes are unavailable, reason from
   known scope, analogous complexity and explicit stages. Both primary seats
   independently propose favorable/planning/adverse durations and reconcile
   material differences. Return numbers with assumptions and uncertainty, not
   blanks or a demand that the operator estimate every activity.

The source draft's assumed 2/6/12/20-hour bands and long bands are **not** defaults.
There is no universal grade-to-time conversion, agent speed multiplier, or fixed
minimum observation count. An adopted locally useful grade band can seed a new
package estimate, but explain departures and preserve its actual basis.

Favorable/planning/adverse are coherent scenarios, not percentile labels. Include
plausible differences in discovery, rework, constraints and availability; do not
just apply an unexplained percentage to the same total. Define the conditions
that make each scenario relevant and report sensitivity to consequential unknowns.
Do not manufacture a probability or average away unresolved disagreement.

## Make each allocation feasible

Identify concrete resources and finite availability: execution seats, independent
review capacity, host memory/build slots, environments, external limits and human
decision/release windows. Use the actual supported model bindings; do not infer
quotas or model capabilities. Declare unknown availability as an assumption or
boundary requiring confirmation, not always-on capacity.

For each scenario:

- Start with pinned canonical dependencies, or the external authority's declared
  graph with limited assurance. A prerequisite outside scheduled scope needs a
  pinned boundary and explicit evidence for its availability; even a completed
  task status is not calendar evidence.
- Allocate discovery/implementation/review/verification/rework, then integration,
  decisions and release where applicable. Working activities consume named
  resources; waiting is an offset or boundary, not fictitious productive effort.
- Fit each non-preemptive interval inside every demanded resource's contiguous
  window. Avoid overlapping demand above capacity. Represent splits explicitly.
  A common reviewer or single build host may serialize otherwise independent work.
- Check all predecessors finish before dependent starts and that release windows
  are represented by actual activities/resources. A capacity ratio or sum of
  grade bands is only a bound, not a feasible schedule. Never add percentiles.
- Run the available read-only checker on the proposed allocation. Its success
  establishes structural/arithmetic consistency only; independently review the
  truth and adequacy of source claims, stage exclusions and resource assumptions.

Separate **resource consumption** (units × working duration), **active elapsed
stages** (including overlaps) and **calendar lead time** (availability/waits).
Show package/category or milestone results at the granularity useful to the
decision. Define any session unit. A realistic multi-day forecast is preferable
to an invented one-session promise; calendar months require actual constraints,
not a default human-team estimation convention.

## Revise and learn without erasing evidence

Freeze forecasts before delivery with source/profile/rubric/binding references,
assumptions and the observation boundary. A material discovery creates a linked
revision with a reason; never replace the initial prediction with an as-built
grade or edit dates until it appears accurate. Preserve all started outcomes:
completed, ongoing, cancelled, failed, reopened and superseded. Ongoing elapsed
time is a lower bound, not a completed observation; cancellation because work was
difficult must not disappear from the record.

Record implementation, independent review, verification, rework, integration,
waiting and human/release outcomes at the declared boundary, using available
evidence. Do not invent missing timings. Keep actual scope and quality failures
separate from original scope to understand error rather than conceal it.

Derive outcome timings rather than recall them. `codeflow estimate outcomes`
reads each completed task's planned, started, blocked, completed and landed
points from git and joins them by task id to the frozen forecasts' planning
scenario; the outcome record cites that report and adds what git cannot show,
such as named waits, failures and reopens. A squash landing or a missing
branch history reports started as unknown; never fill it in. At cold start,
judged durations tend to anchor on human-scale effort: the first three
completed outcomes, or one representative probe, trigger a recorded
recalibration of the judged durations, and the report's line "outcomes
contradict the forecast" is the moment to write a linked revision with its
reason. Its thresholds are printed defaults, not policy; the predeclared
evaluation rule below still decides any claim of usefulness.

Predeclare a prospective evaluation rule before seeing the outcomes: representative
and held-out work, forecast boundary, decision-relevant error and interval width,
handling of unfinished outcomes, uncertainty method and a named simpler baseline
(direct analogue, recent throughput, area/work type). Compare grade usefulness
separately from duration accuracy. A wide interval containing everything is not
a useful win. Report inconclusive or underpowered results honestly; expand data
or simplify the method when it adds no demonstrated decision value.

Use **judgment**, **analogue/probe-supported**, and **locally validated** with
their actual evidence. Reserve **calibrated** for probability claims tested as
probabilities with predeclared tolerances and uncertainty. Scenario fixtures,
deterministic checks, cross-model agreement and a small pilot cannot establish
predictive calibration or a delivery guarantee.

## Methodological context

These sources inform uncertainty, local comparability and schedule checking;
they do not validate this rubric's points or promise agent productivity:

- [SHELF](https://shelf.sites.sheffield.ac.uk/): structured judgment when evidence is sparse.
- [COSMIC](https://cosmic-sizing.org/cosmic-sizing/estimating-with-software-size/): environment-specific size/effort relationships, not transferable human productivity coefficients.
- [GAO Schedule Assessment Guide](https://www.gao.gov/assets/gao-16-89g.pdf): dependency and resource realism.
- [NIST censored observations](https://www.itl.nist.gov/div898/handbook/apr/section1/apr131.htm): incomplete observations need explicit treatment; software modeling adds assumptions.
- [Forecast distribution accuracy](https://otexts.com/fpp3/distaccuracy.html): uncertainty and useful interval width, only when probability definitions warrant those metrics.
