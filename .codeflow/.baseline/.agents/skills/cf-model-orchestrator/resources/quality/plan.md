## Plan fields

Each settled plan records only what a later step reads:

```text
SCOPE_AND_NON_GOALS:
DESIGN_OPTIONS_AND_CHOSEN_DESIGN:
DESIGN_INTENT: <N/A with reason | conform to named system | settled cf-design record>
ASSIGNMENTS:
  TASK_ID | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | CROSS_LINEAGE_REVIEWER seat@effort | DEPENDENCIES
TASK_GRAPH: <multi-task work only>
ACCEPTANCE_CRITERIA_AND_EDGE_CASES:
TEST_PLAN:
SECURITY_NOTE:
APPROVALS: <each seat and the plan version it approved>
```

Approvals name the version they approve. The plan is approved once, for its
shape; a later change needs fresh approval only when it changes the outcome,
a cross-task interface, the dependency graph or a safety boundary.

For multi-task work, `TASK_GRAPH` follows
[the settled task graph contract](../task-graph.md), covers exactly the assigned
tasks, and is part of what the seats approve. In-node execution detail stays
in the ledger under that resource's non-mutation rules.

`DESIGN_INTENT` records `N/A` with the unchanged accepted direction, `conform`
with the named design-system authority, or a settled `cf-design` record; the
[UI and design section](ui-design.md) holds its full rule when a user-facing
surface changes. Research, analysis and planning-only runs fill the remaining
fields as [that section](research-planning.md) sets out.

### Assignments

`RESPONSIBLE_PRIMARY` owns scope, integration, acceptance and the unit's final
accountable verdict. `EXEC_MODE` is `primary-retained`, `same-family-worker`, or
`cross-family-primary`. `EXECUTION` names `binding_id@effort` when a primary
executes, or `route_id@effort` when a worker executes; never invent a worker ID
for retained-primary work. The requested selector and effort are the plan's;
the observed model, effort and native provenance are recorded with the return,
in the PR's review rows. Actual authorship follows the session that produced
the work, not the responsible primary or the transport relay.

The host and both primary seats select the responsible primary, executor and
reviewer from:

1. task fit;
2. required tools and current context;
3. independence from the actual executor's lineage and authored work;
4. verified native availability and routing;
5. observed host resources and bounded concurrency; and
6. observed native usage signals only.

Admissible usage evidence is native usage/status output, actual model/effort or
worker-routing metadata, a scoped live canary, or an explicit harness/rate-limit
error, recorded with its source, time and account or bucket scope when it is
used. Never combine apparently separate limits without evidence or treat one
harness's account state as another's. Unknown remains unknown: never infer
quota, availability, or a worker route from model family, configuration,
elapsed time, silence, or stale telemetry. Unknown usage is advisory while
task fit, capability and quality still decide the route. It blocks only when
the brief declares an explicit hard limit that cannot be evaluated without
that signal. Cost and remaining usage may break a tie among capable routes;
they never excuse a weaker quality gate, prove savings, or silently replace
another criterion.

Propagate current observed unavailability into every later worker choice in the
same task: when a native error or canary proves an exact selector/route
unavailable, record that exclusion and give it to the primary that dispatches
workers, so it does not choose the known-unavailable route again.
Scope it to its evidenced harness, account/bucket, selector/route, and
freshness; do not infer that sibling models or another account are unavailable.
A materially fresh native signal may clear or replace the exclusion.

A change to the responsible primary or cross-lineage reviewer seat or lineage
is a reassignment: it is recorded and reviewed by one other-lineage seat (in
the batched epic amendment for epic tasks) before the affected work continues.
A same-seat trigger-based effort escalation, including direct high to xhigh, is
ledger evidence, not reassignment. Novelty is not a trigger. Mid-session, the
high primary stays the orchestrator and spawns same-family workers at that
effort, each as a native subagent of its own session (the seat section of
`SKILL.md`). Where the harness has no native route for that model, the
recorded fallback is `primary-retained`: keep the high primary, record the
missing route as a limitation, and do not infer a pass. A permitted worker
change within the approved responsible-primary seat remains internal routing
unless it changes the named primary or reviewer.

Default effort is high for primary seats, not a ceiling or a mandate to make
every worker high. A primary already qualified at high can perform suitable
reasoning directly; do not add another high worker merely to satisfy a label.
Delegate substantial, well-specified routine implementation, evidence, or
review-support work to a capable permitted route when one is available and the
unit is separable enough to inspect. Direct primary execution remains valid for
a tiny warm-context change, inseparable unresolved reasoning, material risk, an
unavailable route, or bounded recovery. Record the concrete reason whenever
substantial routine work is retained. Assess demand before the subtask
begins and when new evidence changes its difficulty. Complex architecture and
technical planning use the strongest capable permitted same-family reasoning
route at high or xhigh, selected from the active binding's permitted routes;
same-model routes may add isolation without being cheaper, and a
tool-collection worker is not an equivalent substitute. Routine,
well-specified work stays at the unit's selected worker effort. An xhigh trigger
requires the owning primary to obtain xhigh reasoning through a supported
same-family worker route; do not spend a high attempt merely to fail first.
The primary critically integrates worker findings and retains approvals. If no
required capable route is available, record the unresolved quality gap and
the bounded recovery or degradation rather than claiming medium work
satisfied it.

### Route readiness

The managed ensemble is the only worker-route catalog. `candidate` and
`scoped-qualified` describe evidence status, not native reachability. A
configured candidate is usable for bounded non-design work when current native
routing evidence confirms that exact harness route is currently reachable and
compatible with the required permissions and sandbox. Executable presence alone
is not readiness, and no prior completed workload canary is required: the first
bounded assignment may itself supply start, return, and applied-provenance
evidence under the responsible primary's inspection and acceptance. That does
not make the candidate qualified, cheaper, generally reliable, or applied in a
later run. A configured route without that native readiness evidence is
unavailable for dispatch even if its selector parses.

Candidate reasoning and review-support routes may advise under the same bounded
native-evidence and primary-review rules; they do not own plan, design,
acceptance or independent-review approval. "Qualified" without a `scoped-`
prefix refers to the standing primary-binding contract, not a requirement that
every usable candidate worker already be scoped-qualified.

Qualifying a route, or claiming scoped qualification, promotion or savings,
follows [route status and actual execution](../routing/route-status.md).
