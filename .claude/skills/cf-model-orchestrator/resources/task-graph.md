# Settled task graph contract

Use this contract for multi-task plans. It makes dependencies and decision
points reviewable without turning CodeFlow into a scheduler. The host still
uses its native task tools; the graph is settled plan data and durable task
metadata.

## Nodes and edges

Every assignment row is one outcome-bearing node:

```text
TASK_ID | OUTCOME | PRODUCER | CROSS_LINEAGE_REVIEWER | WRITE_SCOPE | ACCEPTANCE_EVIDENCE
```

Use one directed edge per dependency:

```text
A -> B
```

A bare edge is active unless its source lies on a branch resolved
`not_selected`. Every active bare edge means B cannot start or be accepted
until A has landed with its required task gates green. `when=complete(A)` is
implicit and is never written. Ordering preference alone is not a dependency.

Reserve an evidence guard for a genuine decision settled before execution, or
for evidence beyond ordinary predecessor completion:

```text
PROBE -> DIRECT [when=compatibility:supported]
PROBE -> ADAPT  [when=compatibility:unsupported]
```

The host records the evidence that selected a branch. A guard that merely
restates a standard quality, security, review, or test gate is invalid
duplication and a review finding. An observed outcome that matches no approved
guard is a graph mutation; do not improvise a third branch.

Guarded alternatives are resolved in the approved Plan, not in task metadata.
The matching edge becomes active. Every unselected alternative records
`not_selected` plus the guard evidence in the execution ledger; when a durable
task already exists, its ordinary task status becomes `cancelled` and the
ledger remains the source of the selection reason. A downstream join becomes
eligible only after every alternative guard is resolved and all active bare or
selected guarded predecessors have landed. An ambiguous or unresolved guard
blocks the join and creates Plan vN+1 rather than inviting a guess.

## Well-formed graph

```text
TASK_GRAPH vN
START -> T1
T1 -> T2
T1 -> T3
T2 -> T4
T3 -> T4
```

- Every task assignment appears exactly once as a node.
- Every edge names nodes in the same Plan vN. `START` is the only synthetic
  node and may appear only as a predecessor.
- `START` is plan-only and is never written to task metadata. A root task
  reached from `START` records `depends_on: []`.
- The execution graph is acyclic. Bounded review or rework is a lifecycle loop
  inside a node, not a dependency cycle.
- Tasks with no path between them are eligible for parallel work, not
  automatically parallel. The existing ownership, resource, worktree, shared
  file, and integration rules still decide whether fan-out is worthwhile.
- The integration order is a topological linearization of the graph. Choosing
  another valid linearization is ledger evidence only when ownership, shared
  hotspots, decision guards, and safety remain unchanged.
- A single obvious task records `TASK_GRAPH: N/A (single task)`. Do not create
  a diagram or durable workflow artifact merely to satisfy notation.

Each durable task records its direct non-synthetic predecessors:

```yaml
depends_on: [TSK-003-001]
```

`depends_on` preserves non-executable structural topology. It lists every
direct candidate predecessor, including mutually exclusive guarded candidates
at a later join; Plan guards and ledger evidence own activation and readiness.
Assignment rows own producer/reviewer and the task body owns acceptance
criteria. Do not duplicate those fields into edge labels. CodeFlow accepts the
historical `dependencies` spelling when reading older records, but new work
writes `depends_on`; defining both is invalid. `validate --docs` checks only
identity, references, duplicates, and acyclicity. It never interprets guards,
readiness, completion, or scheduling.

## Mutation and settlement

Create Plan vN+1 and obtain fresh approval from both primary seats before
dependent work continues when evidence requires any of these:

- add, remove, split, or merge a task node;
- add, remove, redirect, or change a dependency edge or decision guard;
- change a node outcome, accepted scope or non-goal, acceptance evidence,
  cross-task interface, or security/recovery boundary;
- change a named producer, cross-lineage reviewer, writer, shared-file owner,
  branch/worktree owner, or lineage;
- change concurrency or integration constraints in a way that alters safe
  isolation, ownership, evidence, or the critical path.

These remain execution evidence inside the approved graph unless they cross a
boundary above:

- steps, commits, focused implementation choices, and bounded rework inside an
  approved node;
- expected-file drift that remains inside the same owned write boundary;
- an extra test that strengthens already-required evidence without changing
  accepted behavior or gates;
- same-seat effort escalation on an approved trigger;
- primary-owned internal worker routing that does not replace a named seat;
- a different valid topological order under unchanged ownership, guards, and
  safety.

Classify **and persist** each such occurrence in the execution ledger with the
supporting evidence. Merely calling it “in-node” or “ledger evidence” without
recording it does not satisfy the trace contract.

Node granularity prevents both evasion and ceremony. A node is a unit that
needs its own outcome and acceptance evidence plus at least one of: a distinct
producer/reviewer assignment, branch/worktree, decision branch, or integration
slot. Work below that threshold is an in-node step. If a supposed step later
needs a different owner, branch, decision branch, or landing slot, it was a new
node: amend the plan before proceeding.

At every node transition, the host verifies predecessor evidence, branch-guard
evidence where applicable, continued conformance to the approved node, and
adequacy of the planned verification. Dispatch or transport completion does not
satisfy an edge.
