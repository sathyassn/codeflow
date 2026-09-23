---
id: ADR-0070
title: "Autonomous operation with judgment"
status: accepted
date: 2026-09-23
supersedes: []
superseded_by: []
architecture_impact: none
---

# ADR-0070: Autonomous operation with judgment

## Context

CodeFlow already defaults to acting: recoverable, task-scoped work is the
agent's, discoverable facts are never questions for the operator, and only a
small set of decisions belongs to the operator. The rule was written seven
ways in seven places (the standard contract's planning and blocker bullets,
the lifecycle reference, the orchestrator preflight, the `cf-plan` clarity
gate, the quality contract and the workspace contract), and no two lists
matched. No text said in one sentence that a task runs to its finish line.
Three places told the agent to stop where it should continue: a change
request could end after its plan, a reversible disagreement between seats
stopped for the human, and a green pull request into an integration branch
waited for a merge the contract already gave to agents. On 2026-09-23 the
operator asked that agents act on judgment and stop only where a human has
to decide, clarify or supply an input.

## Decision

`cf-method/references/autonomy.md` is the one owner of when an agent acts
and when it asks. It carries:

- the finish-line statement: a task runs from its settled outcome through
  build, verification, review, the pull request and its follow-up to a
  readiness report; the agent settles discoverable facts and reversible
  choices from evidence and says what it chose; it stops only at a listed
  gate, and a stop holds only that action;
- the ladder of four rungs: act; notify and act, as one line in the running
  report; ask, with one question, options and a recommendation while other
  work continues; and hard gate, which waits for a human;
- the only full operator-owned list: intent or public behavior the brief
  does not fix, taste, authority or scope beyond the brief, spend,
  credentials, anything sent outside the conversation, a protected-branch
  merge, production, a delete nothing can restore, and any other step with no
  way back;
- the trust prompt rule: the agent answers a trust prompt for a path inside
  its task's own authorized project or worktree, or a disposable sample its
  own harness created in the same run; any other path goes to the operator.
  Authorization and path identity decide what is foreign;
- the settled-dissent rule: after two rounds, a disagreement between seats on
  a reversible choice inside the accepted outcome is settled by the Claude
  judgment primary and recorded as `SETTLED_DISSENT` with both verdicts and
  the evidence. The dissenting verdict is never recorded as approval. A
  dissent on an operator-owned, safety, security or evidence-adequacy axis is
  not settleable;
- a decision table, one row per situation with its source.

The standard contract, `CLAUDE.md`, the lifecycle reference and the
`cf-plan` clarity gate point at the reference. The standard contract keeps
its compact planning list. The minimal tier installs no skill tree, so its
contract carries a compact finish-line clause instead of a pointer. The
hard-gate procedure stays in the contract's "Match the gate to the blast
radius" bullet, and the reference cites it without changing it.

This decision amends one clause of ADR-0035: "Changing the named producer or
reviewer seat or lineage creates Plan vN+1 and requires fresh approval from
both primary seats", together with the joint approval of Plan vN it relies
on. Two cases no longer need both seats' approval. A reversible item carries
settled dissent in place of the dissenting seat's approval. After a recorded
mid-run seat loss, every available standing seat approves the reassignment,
the lost seat is recorded unavailable with reduced assurance, and any verdict
it gave before the loss stays as given. Every other part of ADR-0035 stands.

The record builds on ADR-0034, which escalates only an imminent severe risk
outside scope, on ADR-0038, which forbids reflexive escalation of choices
inside the accepted outcome, and on ADR-0066, which makes an in-tree delete
ordinary work and names the residual risk of a delete that nothing restores.
The reference's delete gate answers that residual.

## Consequences

- An agent has one list to consult, and every other surface points at it, so
  the lists cannot drift apart again. A test asserts the pointer in each
  surface and the absence of a second list in `cf-plan`.
- A change request runs to its readiness report without check-ins. The
  operator sees a running report and is asked only the questions on the list.
- A disagreement between seats on a reversible item no longer stops the work.
  The price is that one seat's objection ships as recorded dissent; review
  and the integrated judgment still see it.
- Seat loss after approval no longer blocks a plan revision. The assurance
  the lost seat would have given is recorded as missing, never simulated.
- The reference adds about 9 KiB to the standard and full tiers. The minimal
  contract stays at its 16 KiB cap by dropping a paragraph that repeated its
  own tier sentence.
- The hard-gate list, the non-relaxable command class and the protected
  merge are unchanged. No new gate is added.

## Alternatives considered

### Keep the lists where they are and align their wording

Rejected. Seven aligned copies still drift at the next edit, and an agent
under pressure still has to reconcile them. One owner with pointers costs
one read.

### Put the full statement and list in the always-loaded contract

Rejected. The standard contract had 2 bytes free under its cap. The detail
belongs on demand, in the method's references, where the lifecycle
reference already lives.

### Leave seat disagreement with the human

Rejected. A disagreement on a reversible choice inside the accepted outcome
is not an operator decision under ADR-0038, and stopping for it is the
over-escalation the operator named. Disagreements on the operator's own axes
and on safety, security or evidence stay closed.

## Architecture impact

None. The scaffold gains one managed reference; no runtime behavior changes.
