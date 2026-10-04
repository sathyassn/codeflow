# Issue handling

Read this when a defect is reported, by an issue, an operator, a reviewer or a
failing check, and before its fix leaves draft. It takes a report to closure
so that a fix covers the whole defect class, a fix loop is redesigned before
it stalls, and a critical defect has a known route. It adds no pull request,
approval or review round: each step writes into the issue or the fix PR.

```text
report -> intake -> cause and class -> sibling sweep -> group by cause
       -> fix the class -> review -> release target -> closure note
```

## The steps

1. **Intake.** The primary reproduces the report with a failing test or one
   bounded probe, or records why it cannot, as the workflow discipline rules'
   "Navigate blockers" says. It judges the report against the critical
   criteria below and says which one holds, or that none does. The
   reproduction and the severity call go in an issue comment.
2. **Cause and class.** Name the mechanism with file:line and the defect
   class: the rule the code broke, in one sentence someone can search for,
   such as "a strict decode of text the operating system supplies" or "a
   substring match over prose". They go in the same comment.
3. **Sibling sweep.** Search the tree for the class and list every site with
   file:line, or write "none found" with what was searched. Every site goes
   in the same unit. When a split reason from the planning rules applies (size,
   a risk boundary, an operator decision), each deferred site gets its own
   issue now, naming the class, never after the fix lands.
4. **Group by cause.** Issues that share a class, or that change the same
   rule, become one unit with one design before anyone branches. A defect in
   a guard, parser, matcher, policy, acceptance rule, hook or CI gets a short
   design first, by the design seat: the rule in one sentence, the cases it
   must accept and refuse, the sites, and what makes the rule checkable. Any
   other fix states its cause and class in the PR Summary.
5. **Fix the class.** Each site gets a regression test that fails before the
   fix and passes after, as "Repair" in
   `cf-model-orchestrator/resources/quality/findings.md` requires. Add one
   durable check for the class where one can be written: a test that walks
   every site, a lint, a doctor check or one rule line. Where none can be
   written, the closure note says why.
6. **Review.** The independent review also checks that the fix covers the
   class, not one site, and that the sweep is recorded. A round that finds a
   new instance of the same class, rather than a regression of the fix, means
   the mechanism is wrong: the rounds stop, the unit returns to step 4 for a
   design, and the PR body records the withdrawn approach. This event is what
   "diagnose a stalled mechanism" means here; it is not a round count.
7. **Release target.** The PR's release impact names the release that
   carries the fix, and an adopter-visible change carries its `Migration`
   line.
8. **Closure note.** The PR closes the issue. The closing comment lists the
   cause, the class, the sites fixed, the sites deferred with their issues,
   the durable check or why there is none, and the release. A deliberate
   choice to leave a site as it is goes here too, where the next reader of
   the issue finds it.

## Critical defects

A defect is critical when it is material, as the review verdict rules define
material, is live in a published release or blocks current work, and does one
of these:

- blocks adopters: a managed file, gate, hook or guard fails or refuses
  ordinary work in consuming projects;
- weakens a security boundary or lets it be bypassed;
- loses or rewrites data, records or history;
- deadlocks or hangs a gate, or blocks the tool's own fix path.

The primary marks the issue critical at intake, with the `critical` label
where the host has labels, and names the criterion that holds. Every other
defect keeps its place in the plan.

The responses, in order. The fix is prioritized when the interim guidance
does not clear the block. Every critical defect then gets a release
decision, and affected adopters are told the outcome, whether or not the
guidance cleared the block.

| Step | What | Who decides |
|---|---|---|
| Interim guidance, the same day | A workaround that is true now, tested before it is posted, with what it does not cover; in the issue, and in the release plan when adopters are affected | the primary |
| Prioritized fix | The fix unit moves ahead of planned work and lands first, so the branch the project releases from stays releasable | the primary, within the fix's own epic; the operator, when it moves another epic's planned work or work the operator ordered |
| Release | The fix goes in the next release the project can cut, and the release plan records whether that release is cut now or waits, and why. Only a route the project's release policy supports; agents never publish | the operator |
| Adopters told | The issue comment, the changelog entry with its `Migration` line and the release notes; a security defect goes through the project's private reporting path, never a public issue | the primary writes, the operator publishes |

The project's release policy names its routes and what each one costs. A
route the policy does not have, such as a maintenance branch, is an operator
decision taken with its guard cost stated, never a step taken under pressure.
