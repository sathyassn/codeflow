## Versioned plan contract

Each settled plan records:

```text
PLAN_VERSION:
BRIEF_DIGEST:
SCOPE_AND_NON_GOALS:
VERIFIED_CONTEXT:
ASSUMPTIONS_OR_UNRESOLVED:
DESIGN_OPTIONS:
CHOSEN_DESIGN_AND_RATIONALE:
DESIGN_INTENT: <N/A with reason | conform to named system | settled cf-design record>
COMPLEXITY_JUSTIFICATION:
TASK_ASSIGNMENTS:
  TASK_ID | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | ROUTING_REASON | ROUTING_EVIDENCE | USAGE | CROSS_LINEAGE_REVIEWER seat@effort | DEPENDENCIES
TASK_GRAPH:
ACCEPTANCE_CRITERIA:
EDGE_AND_ERROR_CASES:
SECURITY_AND_PRIVACY:
TEST_AND_UI_PLAN:
COVERAGE_PLAN:
ROLLBACK_OR_RECOVERY:
CLAUDE_APPROVAL:
CODEX_APPROVAL:
SETTLED_DISSENT: <none | item | both verdicts | evidence | why reversible>
```

Use a content digest or durable link for the immutable brief. Approvals must name
the same plan version. A changed plan invalidates both approvals until each seat
reviews the new version; after a recorded seat loss, the exception in
[task-graph.md](../task-graph.md) says who approves.

After two reconciliation rounds, the Claude judgment primary settles a
disagreement on a reversible choice inside the accepted outcome as
`cf-method/references/autonomy.md` "Settled dissent" allows. `SETTLED_DISSENT`
records the item, both verdicts, the evidence, and why the item is reversible
and settleable. The dissenting seat's verdict on that item stays as given and is
never recorded as approval; that seat must still approve the rest of Plan vN.
Any other open item keeps its gate: an operator-owned item stops only that item
for the operator, and a dissent on an axis that section lists as not settleable
returns to repair.

For multi-task work, `TASK_GRAPH` follows
[the settled task graph contract](../task-graph.md), covers exactly the assigned
tasks, and is part of what both seats approve. A material node, edge, decision
guard, ownership, acceptance/interface, or safety-boundary change creates Plan
vN+1 before dependent work continues. In-node execution detail remains ledger
evidence under the resource's explicit non-mutation rules. A single obvious
task records `TASK_GRAPH: N/A (single task)`.

`DESIGN_INTENT` records `N/A` with the unchanged accepted direction, `conform`
with the named design-system authority, or a settled `cf-design` record; the
[UI and design section](ui-design.md) holds its full rule when a user-facing
surface changes. Research, analysis and planning-only runs fill the remaining
fields as [that section](research-planning.md) sets out.
