## Session roles

Every native session declares one role:

- `host`: the single coordinator that owns the brief, Plan vN, assignments,
  integration, evidence ledger, degradation, and closeout;
- `peer`: a primary cross-lineage reasoning/review seat with plan and approval
  duties; it completes the bounded request and returns evidence;
- `worker`: a bounded native subtask seat owned by its primary; it cannot
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
