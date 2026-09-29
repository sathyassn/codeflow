## Review findings and repair

Read this section when a change fixes a defect, when a review is briefed, and
when review findings are written or acted on. Its review rounds are the
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
The reviewer returns the verdict, the findings with their remedy, what was
verified and what was not verified. Cross-lineage and Herdr briefs follow this
contract.

### Review rounds

A review round runs every assigned reviewer in parallel on one revision. The
builder collects the round's findings into one dependency-ordered batch with
provenance preserved, deduplicates them by mechanism, evaluates each proposed
remedy against the diagnosed mechanism and the impact set, and records accept,
modify or reject with the reason. Rejecting a remedy never closes the finding
or waives a gate; a disputed finding returns with evidence to the reviewer who
raised it. Conflicting remedies are investigated against the mechanism, the
impact evidence and the accepted contract. One consolidated decision goes to
the operator in the departure form under
[blocker navigation](blockers-and-gates.md) only when resolution needs
operator-owned intent, authority or risk acceptance.

Apply the accepted batch as one apply-and-verify cycle (stacked dependents from
Change impact stay separate) and re-verify the impact set. Each reviewer whose
blocker or major finding was fixed confirms that finding on the affected scope,
widened when the impact or the prior evidence is uncertain. A fix for a minor
finding or a nit needs no confirmation round. A round that introduces an
attributable regression is a failed round. The required gates and the
[completion gate](completion.md) are unchanged.

Rounds are bounded by change class. One cycle is repair plus its affected
verification and review.

- Code, configuration and other executable changes: post-review repair is
  bounded to two evidence-moving cycles, and the count carries across route
  changes.
- Docs and records: two review rounds per submitted version; the count resets
  once when an edit changes the duties or claims the text states.

A repeated attempt without a new hypothesis or changed evidence is not another
round. At the bound, diagnose the persistent constraint and take an
approved-outcome-preserving strategic route or surface the genuine external or
owner block, never a third tactical repair.
