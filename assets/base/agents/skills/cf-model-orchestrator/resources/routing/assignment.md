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
same task: when a native error or canary proves an exact selector/route
unavailable, record that exclusion and give it to the primary that dispatches
workers, so it does not choose the known-unavailable route again.
Scope it to its evidenced harness, account/bucket, selector/route, and
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
Claude Code's Agent tool with a subagent definition (see
[Claude worker effort preflight](effort.md)). A Codex
or Grok primary follows the same rule once native evidence shows a subagent
route for that model in its harness; none is recorded yet. Where the harness has no native route for
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
unit's selected worker effort.
The primary critically integrates worker findings and retains approvals; both
families still plan independently and cross-lineage review remains mandatory.
If no required capable route is available, record the unresolved quality gap and
the bounded recovery or degradation rather than claiming medium work satisfied it.

## Route readiness

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
acceptance or independent-review approval. “Qualified” without a `scoped-`
prefix refers to the standing primary-binding contract, not a requirement that
every usable candidate worker already be scoped-qualified.

Qualifying a route, or claiming scoped qualification, promotion or savings,
follows [route status and actual execution](route-status.md).
