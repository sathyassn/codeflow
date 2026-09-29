## Materiality and prioritization

Discovery is broad; action is selective. Finding, substantiating, classifying
severity by consequence apart from confidence, prioritizing, routing, the
critical path and out-of-scope observations are stated once in the workflow
discipline rules, "Find broadly"; the material and nit definitions are in
their "Review verdicts" section. What this section adds for the duo:

Collect only uncertain deferral candidates instead of interrupting the peer
for each observation. The builder gives each nit one disposition in the PR
body: `fix now`, `track once`, or `drop` with the reason. The primary reads
the batch once at the next natural checkpoint (the batch landing or the epic
close); an unavailable seat is recorded as reduced assurance, not silent
agreement. A worthwhile deferral uses one existing tracking altitude (the
epic's planning notes, or the task's follow-ups when there is no epic) with
its evidence and value and a deterministic event trigger such as the next
touch of the surface, a named dependency landing, a named release or quality
gate, or recurrence of the symptom. Age alone, vague "later", one task per
nit, and preference-only backlog entries are invalid.

Dispatch is not disposition. A background task, notification promise, relay
idle signal, or transport completion does not close the batch. The host waits
for the actual bounded peer result and verifies native provenance plus content
before reporting the checkpoint complete.

General reviews retain `blocker | major | minor`: blocker and major findings
lead the report; cosmetic, stylistic, and personal-preference nits are minor and
non-blocking, appear afterward, and do not prevent approval when they are the
only findings. Security reviews retain their CVSS-aligned
`critical | high | medium | low | info` severity and independent confidence;
do not translate that vocabulary inside the security report. When a general
review consumes a security verdict, a confirmed or likely critical/high
security finding is a blocker.
