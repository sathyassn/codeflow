# Evidence-selected verification strength

The normal quality contract always applies. Property/generative tests, mutation
testing, and architecture fitness checks strengthen it when evidence shows they
will expose material risk. They never replace example-based tests, integration
or end-to-end evidence, security review, UI verification, or coverage.

Read this resource once per session. When a technique is selected, name it
and its trigger evidence in the plan's test plan and the task record; when
none is selected, write nothing: no `none selected` line per task. Adding a
triggerless heavyweight technique is avoidable complexity; omitting an earned
technique is brittle under-testing.

Concrete tools, thresholds, commands, and CI cadence belong to the consuming
project. CodeFlow supplies the selection contract, not a universal test stack.

## Property or generative tests

Select them when all of these hold:

1. a stable property or independent oracle can be stated;
2. the meaningful input or state space is broad, combinatorial, or recursive;
3. generation, failure reproduction, and shrinking or minimization are
   controllable; and
4. repository evidence shows that chosen examples may miss material boundary
   combinations.

Strong signals include parsers, serializers and round trips; normalization and
idempotency; ordering, monotonicity, conservation, or algebraic rules;
state-machine invariants; untrusted validation boundaries with a no-panic
contract; and a defect that escaped through an unanticipated input shape.

Keep explicit examples for known singular boundaries and every fixed
regression. Sampling is not proof over an infinite domain and may never hit one
specific rare value.

Do not select property testing for prose or copy, thin getters/wiring, a small
finite contract better enumerated directly, an unstable/non-deterministic
oracle, or a generator more complex and less trustworthy than the production
rule. Do not convert a healthy example suite wholesale for fashion.

## Mutation testing

Use a targeted, time-bounded mutation run when:

- changed guard, branching, validator, policy, state-transition,
  authorization, security, recovery, or other material decision logic needs
  proof that assertions detect a wrong decision;
- a material defect escaped through an otherwise green reliable suite; or
- a project-owned hardening pass names a critical module and a concrete risk.

First require a deterministic base suite. Scope the run to the changed or
critical slice and record the time budget and exclusions. A survived
non-equivalent mutant requires a stronger test or an explicit risk decision;
classify equivalent, unreachable, timed-out, and flaky mutants honestly rather
than gaming a score.

Do not run mutation testing by default for routine or low-risk work, generated
sources, prose, trivial mappings, or an unreliable suite. Do not make a whole
repository mutation score a vanity gate or use it to replace scenario coverage
and review.

## Architecture fitness checks

Add a project-owned deterministic fitness check when:

- a current architecture or product decision states an invariant;
- the invariant has a stable observable rule; and
- a decision record, repeated drift, or high-blast-radius risk explains why
  regression is plausible and material.

Examples include dependency direction, forbidden imports, a single security or
persistence chokepoint, public-schema compatibility, generated/mirror
consistency, and an evidenced performance, size, or resource budget. Prefer an
ordinary test in the project's existing suite when it can express the rule
clearly.

For a cheap in-suite check, the invariant, observable rule, and evidence link
are enough. Add an explicit owner, cadence, threshold, exception process, and
retirement condition in proportion to a heavyweight, cross-team, operational,
or organization-level gate.

Do not encode architectural taste, a speculative future layer, a one-off shape,
an unmeasurable aspiration, or every ADR. Do not duplicate a compiler, type
system, linter, or existing gate unless the new check proves a distinct
decision.

## Review questions

- What material failure could the technique reveal that the normal plan might
  miss?
- Which repository evidence activates the trigger, and which non-trigger was
  considered?
- Is there a smaller deterministic check with equal power?
- Is the generator, mutation tool, or fitness oracle less trustworthy than the
  behavior it judges?
- Is the runtime and maintenance cost proportionate to consequence,
  recurrence, and change rate?
- What reproducible evidence will the producer and cross-lineage reviewer
  retain?
