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
cross-harness task declares `ROLE: peer` or `ROLE: worker`; the prompt limits
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
TASK_ID | PRODUCER seat@effort | CROSS_LINEAGE_REVIEWER seat@effort | ROUTING_EVIDENCE | DEPENDENCIES
```

The host and both primary seats select the producer and reviewer from:

1. task fit;
2. required tools and current context;
3. independence from the producer's lineage and authored work;
4. verified native availability and routing;
5. observed host resources and bounded concurrency; and
6. observed native usage signals only.

Admissible usage evidence is native usage/status output, actual model/effort or
worker-routing metadata, a scoped live canary, or an explicit harness/rate-limit
error. Record its source and observation time. Unknown remains unknown: never
infer quota, availability, or a worker route from model family, config, elapsed
time, silence, or a different harness's state. Cost and remaining usage may
break a tie among qualified routes; they never excuse a weaker quality gate.
If the brief makes a usage signal necessary to decide, a missing signal blocks
assignment until evidence arrives or the operator changes that constraint. Do
not silently substitute another criterion.

A change to the producer or cross-lineage reviewer seat or lineage is
reassignment: create Plan vN+1 and obtain fresh Claude and Codex approval before
work continues. A same-seat medium→high or high→xhigh escalation on a documented trigger is
ledger evidence, not reassignment. Novelty is not a trigger. Mid-session, the
medium primary stays the orchestrator and spawns same-family workers at that
effort. Spawn through the harness's native child-effort knob or a `ROLE:
worker` native session at that effort. Record requested versus observed
selector/effort. If that route is unavailable, keep the medium primary,
record the limitation, and do not infer a pass. A worker change within the approved primary
seat remains internal routing unless it changes the named producer or reviewer.

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

Both adapters — the official plugin lane forward, the schema-v2 delegate
lifecycle reverse — satisfy one five-obligation evidence contract:

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
exposes and has qualified.
Claude owns Claude-side routing and integrated judgment; a Codex host never
selects a Claude worker directly. Concrete selectors, model classes, effort
defaults, and escalation triggers live only in
[current-ensemble.json](current-ensemble.json).
When a catalog family is named, follow
[routing-policy.json](routing-policy.json): default review is the standing
pair; extra-family review requires a named Plan vN assignment when a
documented trigger fires and the family is available, or on operator
instruction, and is never a silent third vote. When a trigger fires, record
available-and-named or unavailable-with-limitation; unknown does not skip
the duty. Claude
produces design in its native session regardless of host.

Treat model names as qualified current bindings, not permanent doctrine.
Record actual model, effort, route, and canary evidence. An unverified worker
route is unavailable, not an invitation to guess or invoke it headlessly.

## Review and degradation

Each producer first-verifies its unit. The named other-lineage seat then reviews
the actual unit and evidence independently. A model cannot independently review
its own authored unit. The model qualified for `claude-judgment-primary` still
reviews the integrated design/code and owns the final Claude quality judgment;
for a unit authored by that primary, record Codex as the independent reviewer
and do not label the primary's integrated judgment an independent unit review.
For product, UX, UI, interaction, or visual design, that role owns intent and
fidelity judgment under `cf-design`. Default UI assignment is Claude as
producer (**implementer check**) and Codex as reviewer (**independent
interactive QA**). Codex QAs through Computer Use on the official app-server
(from a Claude host: plugin/app-server only; from a Grok host or Codex host:
CLI via Herdr if the daemon is missing). If Codex produces a UI unit, Claude
is the independent reviewer and performs Computer Use QA in Claude Code;
Codex producer verification is not independent QA. Playwright remains the
deterministic web driver; Computer Use is not a default web driver and not
design authorship. A worker may collect rendered/tool evidence but never
settles the direction or interprets it in place of the primary. The
`cf-reviewer` subagent may support the implementer check; it does not replace
the Claude primary.

If a planned seat, route, or required tool is unavailable before approval,
select another qualified assignment and settle a new plan version. Mid-run loss
gets one bounded retry and diagnosis; changing a named seat requires
reassignment and fresh approval. If no cross-lineage route remains, use the
documented solo fallback with separate read-only review where possible, record
the missing capability and reduced assurance, and never claim duo completion.
