## Review findings and repair

Read this section when a change fixes a defect, when a review is briefed, and
when review findings are written or acted on. Its review rules are the
working method for every route.

### Repair

A defect fix states the evidenced mechanism, the relevant conditions and the
remaining uncertainty in one sentence, and adds a regression test that fails
before the fix and passes after (Required verification, regression tests for
each fixed defect). The cause counts as found only when the evidenced
conditions are sufficient to produce the failure; reproduce safely where
feasible, otherwise record the strongest evidence and the reproduction limit.
For a defect that resists a first glance, run the probe in
[blocker navigation](blockers-and-gates.md) before hypothesising. Never mask a
violated contract to make a check pass: a catch, fallback, weakened or skipped
test, or retry is a fix only when it implements the contract's required
failure behavior.

### Change impact

This is the full form of the impact rule in
[design and implementation quality](design-implementation.md). List the
candidate impact set (callers, consumers, upstream inputs, downstream effects,
sibling code with the same mechanism, data and schema, configuration, tests,
docs), refine it during verification into the actual impact set, and classify
analogous occurrences by mechanism and materiality. Same-cause occurrences
inside the authorized scope are repaired together; other discoveries route
through `fix now`, `track once` or `drop` under [materiality](materiality.md);
separately authorized work stacks only where a dependency justifies it. Run
the tests of the change's dependents and consumers and of the journey the
change sits in, and verify each same-mechanism sibling repaired with it.

### Findings and remedies

Every blocker or major finding carries the smallest evidenced remedy and its
verification criterion, or, when the remedy is uncertain, the required outcome
and a bounded diagnostic next step; a finding that is an operator decision or
a design question carries the options instead. The reviewer stays read-only,
never writes a redesign or a new `DESIGN_INTENT` to fill the field, and never
reopens settled design without a demonstrated defect. A missing remedy on a
minor is not incompleteness. Blocking is derived from the existing severity,
confidence and gate policy; no second flag. The security reviewer's
`remediation` field is the same duty under its existing name.

A review brief names the unit, its revision, the criteria, the remedy expected
on every blocker and major finding, and the provenance the reply must carry.
It asks for one holistic pass: the whole unit, its full diff against its base
at one head, and its blast radius, meaning what it touches upstream and
downstream, adopters, other platforms, CI time, docs and records. Findings
from earlier rounds enter the brief as checks within that pass, never as its
whole scope. The reviewer returns the verdict, the findings with their remedy,
what was verified and what was not verified. Every review and consult brief,
same-family or cross-family, follows this contract; `cf-herdr` states how a
review seat runs in Herdr.

### Review rounds

Review is one holistic pass per revision: every assigned reviewer reviews the
whole change in parallel on that revision, with no minimum or maximum number
of passes. A round after fixes, or after merging the base, reviews the whole
unit again at the new head, not only the delta; confirming one fix, below, is
not a new round. The builder collects the findings into one dependency-ordered batch
with provenance preserved, deduplicates them by mechanism, evaluates each
proposed remedy against the diagnosed mechanism and the impact set, and
records accept, modify or reject with the reason. Rejecting a remedy never
closes the finding or waives a gate; a disputed finding returns with evidence
to the reviewer who raised it. Conflicting remedies are investigated against
the mechanism, the impact evidence and the accepted contract. One consolidated
decision goes to the operator in the departure form under
[blocker navigation](blockers-and-gates.md) only when resolution needs
operator-owned intent, authority or risk acceptance. Material findings are
fixed in the open task's PR; nits are recorded with a disposition and never
start another pass.

Apply the accepted batch as one apply-and-verify cycle (stacked dependents from
Change impact stay separate) and re-verify the impact set. The finder confirms
each material fix on the affected scope, widened when the impact or the prior
evidence is uncertain. A small fix whose finding came with a failing probe is
confirmed by rerunning that probe and the affected tests, with no new model
turn; a judgment-dependent or widened fix goes back to the finder. Nits need
no confirmation. A cycle that introduces an attributable regression is a
failed cycle. The required gates and the [completion](completion.md) section
are unchanged.

Review ends on evidence: every criterion not marked deferred has evidence on
the reviewed revision, the needed checks are green, no material finding is
open and every nit has a disposition. Continue while repairs produce relevant
evidence; diagnose a stalled mechanism, an invalid assumption or a materially
changed scope, then split, redesign or take the intent question to the
operator. That decision is never made by a round counter and never by
automatic acceptance. A repeated attempt without a new hypothesis or changed
evidence is not progress.
