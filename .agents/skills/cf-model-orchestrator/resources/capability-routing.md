# Capability-routing contract

This resource defines who may produce and independently review each approved
task. It changes execution assignment, not the duo's independent discovery,
Claude-led design, versioned joint approval, evidence gates, or safety boundary.

## Session roles

Every native session declares one role:

- `host` — the single coordinator that owns the brief, Plan vN, assignments,
  integration, evidence ledger, degradation, and closeout;
- `peer` — a primary cross-lineage reasoning/review seat with plan and approval
  duties; it completes the bounded request and returns evidence;
- `worker` — a bounded native subtask seat owned by its primary; it cannot
  approve the plan or replace a named reviewer.

Only `host` invokes `cf-model-orchestrator`. The first line of every
cross-family task declares `ROLE: peer` and invokes the receiving family's
qualified primary at its default effort. `ROLE: worker` is only for same-family
work owned and dispatched by that family's primary, as a native subagent of
that primary's own session, never a separate CLI session or Herdr tab. The
prompt limits
the session to that bounded assignment and explicitly forbids starting the
top-level orchestrator or delegating back to the host lineage. A generic
same-lineage subagent cannot satisfy a named cross-lineage assignment. A Grok
Build host coordinates the standing pair through Herdr; it does not start a
nested duo. Hermes and other non-catalog harnesses normally hand the whole
repository task to one native CodeFlow host instead of becoming an outer host
around a second inner duo.

## Assignment record

Plan vN records one row per task:

```text
TASK_ID | RESPONSIBLE_PRIMARY seat@effort | EXEC_MODE | EXECUTION | ROUTING_REASON | ROUTING_EVIDENCE | USAGE | CROSS_LINEAGE_REVIEWER seat@effort | DEPENDENCIES
```

`RESPONSIBLE_PRIMARY` owns scope, integration, acceptance and the unit's final
accountable verdict. `EXEC_MODE` is `primary-retained`, `same-family-worker`, or
`cross-family-primary`. `EXECUTION` names `binding_id@effort` when a primary
executes, or `route_id@effort` when a worker executes; never invent a worker ID
for retained-primary work. `ROUTING_EVIDENCE` separates requested from observed
selector, effort and native provenance. Actual authorship follows the session
that produced the work, not the responsible primary or the transport relay.

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
error. `USAGE` records the source and observation time, harness, account or
bucket scope, any shared-bucket relationship, remaining/reset values only when
exposed, and the freshness judgment for this decision. Never combine apparently
separate limits without evidence or treat one harness's account state as
another's. Unknown remains unknown: never infer quota, availability, or a
worker route from model family, configuration, elapsed time, silence, or stale
telemetry. Unknown usage is advisory while task fit, capability and quality
still decide the route. It blocks only when the brief declares an explicit hard
limit that cannot be evaluated without that signal. Cost and remaining usage
may break a tie among capable routes; they never excuse a weaker quality gate,
prove savings, or silently replace another criterion.

Propagate current observed unavailability into every later worker choice in the
same task. When a native error or canary proves an exact selector/route
unavailable, record that exclusion and give it to the primary that dispatches
workers so it does not choose the known-unavailable route again. Scope the
exclusion to its evidenced harness, account/bucket, selector/route, and
freshness; do not infer that sibling models or another account are unavailable.
A materially fresh native signal may clear or replace the exclusion.

A change to the responsible primary or cross-lineage reviewer seat or lineage is
reassignment: create Plan vN+1 and obtain fresh Claude and Codex approval before
work continues. A same-seat trigger-based effort escalation, including direct
high→xhigh, is ledger evidence, not reassignment. Novelty is not a trigger. Mid-session, the
high primary stays the orchestrator and spawns same-family workers at that
effort. Same-family workers run inside the host harness: spawn each one as a
native subagent of the primary's own session through the harness's native
child-effort knob, never as a separate CLI session or Herdr tab. A Claude
primary, in the desktop app or the CLI alike, launches Fable or Opus through
Claude Code's Agent tool with a subagent definition (preflight below). A Codex
or Grok primary follows the same rule once native evidence shows a subagent
route for that model in its harness; none is recorded yet. Record requested
versus observed selector/effort. Where the harness has no native route for
that model, the recorded fallback is `primary-retained`: keep the high
primary, record the missing route as a limitation, and do not infer a pass.
A permitted worker change within the approved responsible-primary seat remains
internal routing unless it changes the named primary or reviewer.

Default effort is high for primary seats, not a ceiling or a mandate to make
every worker high. A primary already qualified at high can perform suitable
reasoning directly; do not add another high worker merely to satisfy a label.
Delegate substantial, well-specified routine implementation, evidence, or
review-support work to a capable permitted route when one is available and the
unit is separable enough to inspect. Direct primary execution remains valid for
a tiny warm-context change, inseparable unresolved reasoning, material risk, an
unavailable route, or bounded recovery. Record the concrete reason whenever
substantial routine work is retained. Assess demand before the subtask begins
and when new evidence changes its difficulty. Complex architecture and
technical planning use the strongest capable permitted same-family reasoning
route suited to that unit at high or xhigh, selected from the active
binding's permitted routes. Same-model routes may add isolation or
specialization without being cheaper. A tool-collection worker is not an equivalent
substitute for that reasoning. An xhigh trigger requires the owning primary to
obtain xhigh reasoning through a supported same-family worker route; do not spend
a high attempt merely to fail first. Routine, well-specified work stays at the
unit's selected worker effort without automatically escalating every worker.
The primary critically integrates worker findings and retains approvals; both
families still plan independently and cross-lineage review remains mandatory.
If no required capable route is available, record the unresolved quality gap and
the bounded recovery or degradation rather than claiming medium work satisfied it.

## Route status and actual execution

The managed ensemble is the only worker-route catalog. `candidate` and
`scoped-qualified` describe evidence status, not native reachability. A
configured candidate is usable for bounded non-design work when current native
routing evidence confirms that exact harness route is currently reachable and
compatible with the required permissions and sandbox. Executable presence alone
is not readiness, and no prior completed workload canary is required. The first
bounded assignment may itself supply start, return, and applied-provenance
evidence under the responsible primary's inspection and acceptance. Requested
selector/effort remains requested until the public native surface exposes the
applied values. This does not make the candidate qualified, cheaper, generally
reliable, or applied in a later run. Conversely, a configured route without that
native readiness evidence is unavailable for dispatch even if its selector
parses.

`scoped-qualified` adds reviewed outcome evidence only for the declared
harness, selector, effort and workload tuples. Evidence paths are relative to
the repository that owns the catalog and its source evidence; consuming
projects neither resolve them locally nor duplicate an evidence database.
Before launch, pre-register the cases and label each arm `qualifying` or
`comparison`; never reclassify an arm after seeing results. Before a status
change, the route/tuple being qualified must provide three fresh accepted trials
for every pre-registered case and qualifying arm, complete applied identity and
trace evidence, primary inspection/integration, and cross-family review.
Comparison outcomes inform claim scope but do not themselves gate qualification.
Any failed qualifying acceptance, missing applied identity or trace, invalid
control/fixture, or unresolved validity threat leaves the route candidate.
Small successful cohorts support only their narrow claim. Full
primary-binding promotion still requires the existing complete suite and
approved binding record. Savings or economical-default recommendations
separately require measured all-attempt capacity, time or cost benefit including
coordination and rework; route status alone supplies none.

Candidate reasoning and review-support routes may advise under the same bounded
native-evidence and primary-review rules; they do not own plan, design,
acceptance or independent-review approval. “Qualified” without a `scoped-`
prefix refers to the standing primary-binding contract, not a requirement that
every usable candidate worker already be scoped-qualified.

## Claude worker effort preflight

Absence of an effort parameter on the Agent tool is not proof that Claude
cannot run stronger workers. Check the installed version's supported
[subagent definitions](https://code.claude.com/docs/en/sub-agents): `effort`
frontmatter or a session-scoped `--agents` JSON definition can set a child's
effort independently of the primary. At launch, define only the bounded worker
needed, with `description`, `prompt`, permitted `model`, and `effort`; the
owning Claude primary invokes that named `subagent_type`. In an existing
session, verify a supported definition is loaded before invoking it. Never
invent a missing Agent argument or install a permanent fleet of worker roles.

Before launching any Claude worker, **read and follow**
`.claude/skills/cf-delegate/resources/claude-turn-completion.md`, especially
"Sequential turns." The dispatch must keep worker collection inside the
accepted foreground turn: collect the worker result before the primary returns,
do not use background Bash watchers or task notifications as completion, and do
not accept a terminal response that says work is still running. Claude Code
backgrounds subagents by default, so when the turn needs the result, launch the
worker with the Agent tool's `run_in_background: false` and collect its return
in that turn; a backgrounded worker's result counts only when collected from
that same launch before the turn reports completion. If the harness cannot
provide that supported foreground return, the route is unavailable for
this dispatch; use another bounded native route and preserve the existing
Stop-hook and lifecycle safety policy unchanged.

Keep the primary at its default effort. Inspect effective
[effort precedence](https://code.claude.com/docs/en/model-config):
`CLAUDE_CODE_EFFORT_LEVEL` can override the child's definition, and supported
levels depend on the selected model and organization limits. Do not change
global settings or blanket defaults to make one worker stronger. Resolve an
override only within an authorized task process, retaining the primary's
explicit default; otherwise record the limitation. Use an approved binding's
route and a bounded native canary. Definition values remain requested until
native metadata verifies the model/effort; self-report is insufficient.

## Admissible cross-lineage evidence

Output attributed to the other lineage is admissible cross-lineage evidence
only with native runtime provenance: the vendor session/thread/task id plus
model and effort labeled by their actual evidence source (`observed` when the
transport exposes actual values, otherwise `requested`). Unproven attribution
is reclassified as the author seat's own lineage and the cross-lineage half of
the work is redone.
A relay — plugin, adapter, relay subagent, or transport session — is transport,
not author. A same-lineage worker that produces work remains same-lineage; a
relay answering in the other vendor's name is evidence fabrication, and no
seat may simulate a missing vendor.

Every qualified native route — preferred plugin, official client fallback,
or schema-v2 delegate lifecycle — satisfies one five-obligation evidence contract:

1. **Launch** — verify the delegated task started through a native session
   artifact: a Codex thread forward or a lifecycle ready record reverse;
2. **Provenance** — native runtime provenance only: a native Codex thread ID
   with source-labeled model/effort forward; the lifecycle's
   session/digest/prompt binding reverse;
3. **Return** — verify the returned unit, scoped worktree diff, and cited
   evidence; a relay's idle or completion signal is evidence of neither;
4. **Failure** — a legible bounded failure (stable exit state, durable
   poison, or explicit harness error), never silent substitution or
   completion inferred from silence;
5. **Recheck** — evidence recheckable through the native surface after the
   fact: the resumable Codex thread forward, the durable state records until
   cleanup reverse.

Record model/effort as observed only when the transport exposes actual values;
otherwise label them requested, and never silently upgrade requested to
observed. Grade inferred completion explicitly as inferred.

## Native host routes

The durable route is primary-owned: the active primary coordinates at the
recorded default effort, stays the orchestrator, invokes the other-lineage
primary directly, and may use only the bounded internal routes its own seat
exposes under the status and native-evidence rules above.
Each primary owns its family's routing: a caller never selects a foreign
worker directly or passes worker escalation effort on the foreign primary's
entry command. Instead, send complexity and the required outcome to that
primary; it chooses its own permitted worker when appropriate and reviews the
return.
Later primary-approval prose cannot repair an incorrect initial dispatch.
Claude retains integrated judgment. Concrete selectors, model classes, effort
defaults, and escalation triggers live only in
[current-ensemble.json](current-ensemble.json).
When a catalog family is named, follow
[routing-policy.json](routing-policy.json): default review is the standing
pair; extra-family review requires a named Plan vN assignment when a
documented trigger fires and the family is available, or on operator
instruction, and is never a silent third vote. When a trigger fires, record
available-and-named or unavailable-with-limitation; unknown does not skip
the duty. The Claude design owner produces direction and real design
implementation in its native session regardless of host. While no matching
Claude route is scoped-qualified for `design-implementation`, the Claude
primary executes that work directly; a candidate design route is limited to a
controlled disposable qualification fixture. A scoped-qualified route may
execute only settled evidenced tuples and never owns direction or fidelity
approval. The ensemble's recorded same-Claude primary fallback retains the same
Claude authority after native preflight when the preferred primary is
unavailable. Another family may design only when an explicit
operator instruction names that task-specific override in Plan vN; Claude
absence alone is not an override.

Treat model names as current catalog bindings, not permanent doctrine. Record
actual model, effort, route, and canary evidence. An unverified worker route is
unavailable, not an invitation to guess or invoke it headlessly; candidate
status alone is not that unavailability evidence.

## Review and degradation

Each actual executor first-verifies its unit. The responsible primary inspects,
integrates and accepts the return without secretly duplicating it. The named
other-lineage seat then reviews the actual unit and evidence independently;
review lineage is opposite the session that authored the work, even when a
different primary remains accountable. A model cannot independently review its
own authored unit. The model qualified for `claude-judgment-primary` still
reviews the integrated design/code and owns the final Claude quality judgment;
for a unit authored by that primary, record Codex as the independent reviewer
and do not label the primary's integrated judgment an independent unit review.
Retain every lineage that contributes to a mixed diff, review each authored
unit from another lineage, and inspect the integration; neither contributor
independently reviews its own contribution. A recovery replacement that fully
discards an earlier attempt follows current authorship alone and does not create
a blanket third-family requirement.
For product, UX, UI, interaction, or visual design, that role owns intent,
direction, implementation/execution and fidelity judgment under `cf-design`,
subject only to the scoped-qualified Claude route and explicit operator-override
rules above. Default UI assignment is Claude as responsible primary and
executor (**implementer check**) and Codex as reviewer (**independent interactive
QA**). Codex QAs through Computer Use on the official app-server
(preferred plugin or qualified official native client per `cf-delegate`,
with actual Computer Use access verified). If Codex produces a UI unit, Claude
is the independent reviewer and performs Computer Use QA in Claude Code;
Codex executor verification is not independent QA. Playwright remains the
deterministic web driver; Computer Use is not a default web driver and not
design authorship. A non-design worker may collect rendered/tool evidence but
never settles the direction, authors the design, or interprets it in place of
the primary. The
`cf-reviewer` subagent may support the implementer check; it does not replace
the Claude primary.

If a planned seat, route, or required tool is unavailable before approval,
select another qualified assignment and settle a new plan version. Mid-run loss
gets one bounded retry and diagnosis; changing a named seat requires
reassignment and fresh approval. If no cross-lineage route remains, use the
documented solo fallback with separate read-only review where possible, record
the missing capability and reduced assurance, and never claim duo completion.
