# Independent review

The approved reviewer from a lineage different from the actual author's
reviews the actual unit, reruns relevant gates, and checks conformance with the
chosen design, after the executor and responsible primary steps in
[capability-routing](../capability-routing.md). Every reviewer challenges the
evidence rather than accepting a summary.
For a mixed-authorship diff, retain every contributing lineage in provenance,
review each authored unit from a different lineage, and then inspect the
integration. Neither contributor's pass is independent review of its own
contribution. When recovery fully discards an earlier attempt, current
authorship alone governs review; do not add a blanket third-family ceremony.

Classify findings by severity, order them by [materiality](materiality.md),
and support each with a concrete trigger or reproduction plus its priority
rationale. Security approval requires checking untrusted inputs through
their sinks, authentication/authorization, secrets and privacy, dependency
risk, injection, path/process boundaries, and the agent-facing
prompt/instruction surface where present.
