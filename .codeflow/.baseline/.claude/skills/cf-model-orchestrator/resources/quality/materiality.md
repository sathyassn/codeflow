## Materiality and prioritization

Discovery is broad; action is selective. Use this sequence for code, design,
documentation, research, operations, and proactive observations:

1. **Find** candidate problems, risks, and opportunities without filtering for
   what is easiest to fix.
2. **Substantiate** each candidate with a concrete trigger, source, reproduction,
   or observable consequence. A hunch may guide investigation but is not yet a
   finding.
3. **Classify severity** by the consequence if the issue remains unresolved.
   Keep evidence confidence separate from consequence.
4. **Prioritize action** from severity plus confidence, likelihood or
   reachability, blast radius, urgency or cost of delay, recurrence or systemic
   leverage, and dependencies. State that rationale with the finding.
5. **Route** it to the current work, immediate escalation, one tracked follow-up,
   or a clearly non-blocking batch.

Materiality identifies consequence and value; the critical path identifies the
current dependency or blocker controlling the accepted outcome. For multi-step
work, keep that focus explicit, allocate capable attention, tools, and bounded
resources there, and reassess it when evidence, dependencies, blockers,
integration, or gates change. Allied work belongs in the current run when it
unblocks or de-risks that path, satisfies this quality contract, or is a clear,
safe, local, in-scope improvement with bounded validation. Critical-path focus
never waives accepted quality, testing, security, review, documentation, or
recovery, and it never licenses unrelated bundling.

Do not postpone an earned improvement merely because it is secondary. Fix and
verify it while context is warm when the conditions above hold. Collect only
uncertain deferral candidates instead of interrupting the peer for each
observation. At the next natural cross-lineage review or closeout checkpoint,
both primary seats inspect the consolidated batch and choose `fix now`, `track
once`, or `drop`; an unavailable seat is recorded as reduced assurance, not
silent agreement. A worthwhile deferral uses one existing tracking altitude
with its evidence/value and a deterministic event trigger such as the next
touch of the surface, a named dependency landing, a named release/quality gate,
or recurrence of the symptom. Age alone, vague "later", one task per nit, and
preference-only backlog entries are invalid.

Dispatch is not disposition. A background task, notification promise, relay
idle signal, or transport completion does not close the batch. The host waits
for the actual bounded peer result and verifies native provenance plus content
before reporting the checkpoint complete.

Remediation effort is planning input only. It may change sequence or ownership;
it never lowers severity or justifies choosing an easy cosmetic change over a
material one. Repeated minor symptoms may be evidence of one major systemic
cause, so investigate the pattern before reporting a pile of isolated nits.

General reviews retain `blocker | major | minor`: blocker and major findings
lead the report; cosmetic, stylistic, and personal-preference nits are minor and
non-blocking, appear afterward, and do not prevent approval when they are the
only findings. Security reviews retain their CVSS-aligned
`critical | high | medium | low | info` severity and independent confidence;
do not translate that vocabulary inside the security report. When a general
review consumes a security verdict, a confirmed or likely critical/high
security finding is a blocker.

Do not silently absorb out-of-scope work. An evidenced imminent severe risk is
escalated immediately; another material observation becomes one tracked item
with evidence and a proposed route. Isolated nits are noted or batched, not
turned into one issue each. No external mutation or scope expansion follows
from discovery without the authority required by the task. "Nothing material
found" is a valid result; issue farming and fabricated proactive signals are
failures.
