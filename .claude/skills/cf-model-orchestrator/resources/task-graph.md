# Settled task graph contract

Use this contract for multi-task plans. It makes dependencies and decision
points reviewable without turning CodeFlow into a scheduler. The host still
uses its native task tools; the graph is settled plan data and durable task
metadata.

## Nodes and edges

Every assignment row is one outcome-bearing node and uses the canonical
responsibility and execution fields of the plan's assignment line
(`quality/plan.md`):

```text
TASK_ID | OUTCOME | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | AUTHORSHIP | CROSS_LINEAGE_REVIEWER seat@effort | WRITE_SCOPE | ACCEPTANCE_EVIDENCE
```

Use one directed edge per dependency:

```text
A -> B
```

A bare edge is active unless its source lies on a branch resolved
`not_selected`. Every active bare edge means B cannot be accepted or land
until A has landed with its required gates green. B may start earlier only on
A's exact reviewed head, named with `--on TSK-A@<sha>` to `codeflow work claim`
and `work start`; A still lands first, and a change to A after its review
means rebase on its new reviewed head and recheck. `when=complete(A)` is
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

Guarded alternatives are resolved in the approved Plan, and the matching edge
becomes active. Only the selected branch of a decision is written into
`depends_on`. Until the selection is approved on the target, the join task
carries `awaiting_selection: <plan or decision path>`, is `blocked` with the
reason "awaiting selection", and may have an empty `depends_on`. The selection
lands only by the batched epic amendment that removes `awaiting_selection`,
writes the selected dependencies and unblocks the join; it never lands on a
task branch.
Every unselected alternative records `not_selected` plus the guard evidence in
the execution ledger; when a durable task already exists, it is cancelled, and
a cancelled, unselected alternative never blocks a join. An ambiguous or
unresolved guard blocks the join and creates a new plan version rather than
inviting a guess.

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
- The execution graph is acyclic. Review and rework are a lifecycle loop
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
depends_on: [TSK-003, {id: TSK-002, kind: research, pin: "<commit sha>"}]
```

`depends_on` preserves non-executable structural topology: every direct
predecessor on the selected branch, each a bare task id for a code dependency
or `{id: TSK-NNN, kind: research | decision, pin: "<commit sha>"}` for an
input read at a pinned commit. The work lifecycle in `cf-method` says when
each is met; Plan guards and ledger evidence own activation.
Assignment rows own responsible primary, actual execution and reviewer; the
task body owns acceptance criteria. Do not duplicate those fields into edge
labels. CodeFlow accepts the
historical `dependencies` spelling when reading older records, but new work
writes `depends_on`; defining both is invalid. `validate --docs` checks
identity, references, duplicates, acyclicity, dependency shape, and that an
`awaiting_selection` path exists. It never interprets guards, readiness,
completion, or scheduling.

## Mutation and settlement

Create a new plan version, approved by both primary seats before dependent
work continues, only when evidence changes one of these:

- a node's outcome, or the epic's accepted outcome, scope or non-goals;
- a cross-task interface;
- the dependency graph: a dependency edge, a decision guard, or a node whose
  addition or removal changes what other nodes depend on;
- a security or recovery boundary.

A reversible item in that approval may carry `SETTLED_DISSENT` under the
[quality contract](quality-contract.md) plan record.

These ride in one batched epic amendment on a `plan/` branch, reviewed by one
other-lineage seat:

- reassignment of a named responsible primary, reviewer, task or file owner,
  or branch and worktree owner;
- merge or split of unstarted tasks that keeps their outcomes and edges;
- follow-ups, re-sizing, and criteria changes of other tasks;
- a concurrency or integration constraint change that keeps isolation and
  safety.

One amendment may span several epics: its PR names each on one `Task:` line
(`Task: EPC-001, EPC-002`) and may carry docs and the project section of
`AGENTS.md`; CI reports each change per epic and refuses a change to an epic
the line does not name (ADR-0078). It lands once on `main`, and each
integration line takes it by merging `main`.

A task's own criteria change rides in its own PR, where CI prints the change
for the reviewer.

These remain execution evidence inside the approved graph:

- steps, commits, focused implementation choices, and rework inside an
  approved node;
- expected-file drift that remains inside the same owned write boundary;
- an extra test that strengthens already-required evidence without changing
  accepted behavior or gates;
- same-seat effort escalation on an approved trigger;
- primary-owned internal worker routing, including a permitted executor or
  worker-effort change, that preserves the named seats, authored-lineage review,
  task/file ownership, scope and isolation boundary;
- a different valid topological order under unchanged ownership, guards, and
  safety.

Log material dependency or decision changes and meaningful checkpoints in the
execution ledger with their evidence; routine in-node steps need no entry.

A node is one outcome a user or operator can observe and verify, worth its own
review and landing, delivered in one PR. Its floor is a **narrow complete
path** that is demoable or verifiable on its own (schema through the exercised
surface plus tests), not a horizontal layer-slice. Wide mechanical refactors
are the exception: expand the new form beside the old, migrate callers in
blast-radius batches, then contract the old form; do not force them into a
fake vertical slice. Split a node only for a reason written in the task: value
that can ship alone, a contract boundary a consumer needs pinned, an operator
decision that gates part of it, a size too big for one thorough review, or a
risk boundary (security, data, irreversible action). A branch, a worker, a
file owner, a reviewer seat, a landing slot or a review finding is never a
reason to create or split a node; work that is not its own outcome is combined
with the outcome it serves. A node typically has 3 to 8 criteria; a broad
safety change may need more, and past about a dozen ask whether it is two
outcomes. These numbers are orientation, never a split rule. If a supposed
step later turns out to be a separate outcome, it is a new node: add it in the
batched epic amendment before proceeding.

A large epic may record **Not yet specified** (in-scope fog that cannot yet be
phrased as a node) versus **Out of scope** (ruled beyond this destination).
Ticket when the question is already sharp, even if blocked. That is planning
notes on the epic, not a second work tracker.

At every node transition, the host verifies predecessor evidence, branch-guard
evidence where applicable, continued conformance to the approved node, and
adequacy of the planned verification. Dispatch or transport completion does not
satisfy an edge.
