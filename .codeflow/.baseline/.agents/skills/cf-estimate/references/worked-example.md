# Worked example: one read contract

This is a fictional calculation example, not evidence of CodeFlow productivity
or a reusable hour band. Its numbers illustrate a declared scope, explicit
resource allocation and uncertainty. A real forecast must be based on the
consuming project's evidence and reviewed assumptions.

## Scope and classification

A package changes one existing read API's filtering contract, using an unchanged
data store and implementation already owned elsewhere. Its brief defines inputs,
empty results and three distinct refusal obligations. No UI, owned computation,
new authority, migration or external conformance is in scope. Review and
verification include the real endpoint and its backing-service integration.

Pinned anchors support S2=1; S1/S3–S7/C/X/T=0; R=0 (three distinct obligations).
Total 1: **Easy**. Consumed storage and engine surfaces score zero, but integration
tests and error handling still take time. Calling the three refusals
“invariants” instead must not alter R. Discovering a new persisted shape would
change scope and require a revised anchored grade, not a silent date adjustment.

## Explicit judgment at cold start

Assume a qualified implementation seat and an independent reviewer, one shared
build environment, a defined small scope, and no comparable local history. The
following active durations are deliberately illustrative judgment values:

| Stage | Favorable | Planning | Adverse |
|---|---:|---:|---:|
| Implement | 30 min | 50 min | 80 min |
| Independent review | 15 min | 25 min | 40 min |
| Verify endpoint and backing service | 20 min | 35 min | 55 min |
| Expected rework and recheck | 10 min | 25 min | 45 min |
| Integrate | 10 min | 15 min | 25 min |
| Release activity | 5 min | 10 min | 15 min |
| Serial active elapsed | 90 min | 160 min | 260 min |

Favorable assumes the filtering implementation is directly reusable; planning
allows one meaningful review correction; adverse allows a discovered interaction
with the backing-service fixture and a second verification cycle. These are
scenario assumptions, not P10/P50/P90 or guarantees. A material unknown about
who owns the filtering behavior would instead make the package UNSIZED until
resolved; ordinary duration uncertainty does not.

For a fully serial allocation beginning at 09:00, the planning active work ends
at 11:40 if all resources are available. If release access first opens at 13:00,
the release waits until then and finishes at 13:10: **4h10 calendar lead time**,
not 2h40. The gap is not implementation effort. A reviewer occupied until 11:00
can extend the critical path further; recalculate actual intervals rather than
divide the total by two because two agents exist.

An overlapping second package may use the implementation seat during review,
but only with separate files/worktrees, enough host memory and no competing
build/environment demand. The checker must reject an allocation that occupies
the sole build environment twice. Its green output still cannot establish that
the assumed timings, reviewer availability or acceptance coverage are true.

## Forecast revision and observation

Freeze the scope/profile/rubric pins and these assumptions before actual delivery.
Suppose a real project later discovers a required backing-service behavior change:
preserve v1, record the discovery and produce v2 through the same review boundary.
Do not replace v1's Easy grade with the as-built grade to hide estimation error.
Record observed review/rework/waiting separately, including failure, cancellation
or reopening. One example or one delivered package establishes neither a useful
grade-to-duration relationship nor predictive calibration.
## Run the complete fixture

The distributed [forecast.json](../examples/forecast.json) includes all three
scenarios, explicit resource windows, stage allocations, a milestone and SHA-256
pins for its fictional profile, brief and the distributed rubric. From the root
of a standard/full project with the updated checker installed:

```sh
codeflow estimate check .agents/skills/cf-estimate/examples/forecast.json --json
```

The mirrored Claude copy uses the same project-relative `.agents/` source pins;
both namespaces are installed together. Expected elapsed totals are 5,400,
9,600 and 15,600 seconds. Source assurance is `limited`: local fictional inputs
are pinned, not independently validated project commitments. This fixture uses
an arbitrary zero epoch and eight-hour windows; it does not consult today's
calendar or write adoption records. The later release-window variation described
above is a separate sensitivity example, not secretly included in this file.
Canonical-source and boundary checks are exercised in the checker's tests, not
this single-package fixture; apply the source rules in records.md for real work.

Do not edit managed examples into live forecasts. After confirmed adoption,
create a project-owned snapshot, replace every fictional premise and source pin,
and retain the original prediction when revising it. A source edit that invalidates
a pin must fail; do not claim historical inputs stayed unchanged.
