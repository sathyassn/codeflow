---
id: ADR-0035
uid: 2c4c815c-d63e-4718-a68b-a41066f78481
title: route duo execution by verified per-task capability
date: 2026-07-22
status: accepted
superseded_by: null
architecture_impact: scaffold orchestration gains per-task producer/reviewer assignments and explicit host/peer/worker roles; the binary remains model agnostic
---

# ADR-0035: capability-routed duo execution

## Context

The host-neutral duo correctly requires independent Claude+Codex discovery,
Claude-led design, versioned joint plan approval, native interactive sessions,
and evidence gates. Its execution rule is unnecessarily fixed, however:

- ADR-0023 lines 59–66 always assign implementation/first verification to
  Codex and independent diff review to Claude;
- ADR-0024 lines 35–52 carry that fixed binding into every implementation mode;
- ADR-0028 lines 38–43 retain Codex implementation ownership while introducing
  evidence-routed workers; and
- ADR-0030 lines 24–37 bind proportionality verification to the same fixed
  producer/reviewer arrangement.

The fixed split preserves cross-vendor independence, but it can route a task
away from the seat with the better native tool, context, or qualified worker.
It also makes Fable's integrated judgment appear independent even for a unit
Fable authored. Runtime usage can affect a qualified choice, but only when the
harness actually exposes it; inferred quota is fabricated evidence.

## Decision

Preserve the duo's independent discovery, Claude design leadership, versioned
joint approval, quality/safety gates, native interactive transport, and Fable
integrated quality judgment. Supersede only the fixed producer and fixed
review-independence clauses identified above.

After both seats independently plan, the host records every implementation task
in Plan vN with a producer seat@effort, a cross-lineage reviewer seat@effort,
routing evidence, and dependencies. Selection considers task fit, tools and
current context, reviewer independence, verified native availability/routing,
host resources, and observed native usage signals only. Admissible usage
evidence is native usage/status output, actual route metadata or a scoped
canary, and explicit harness/rate-limit errors. Unknown remains unknown; model
family, configuration, elapsed time, silence, and another harness's state do not
prove availability or quota.

Changing the named producer or reviewer seat or lineage creates Plan vN+1 and
requires fresh approval from both primary seats. A same-seat high-to-xhigh
escalation on an already-approved trigger is recorded evidence, not
reassignment. Each producer first-verifies its unit and the named other-lineage
seat independently reviews the actual unit. Fable still reviews the integrated
design/code and owns Claude's final quality judgment. For a Fable-authored unit,
Codex is the independent reviewer and Fable's integrated pass is not described
as independent unit review.

Every native session declares one role. The `host` is the sole top-level
coordinator; a `peer` retains primary plan/review duties; a `worker` performs a
bounded subtask under its primary. Peers and workers never recursively invoke
the orchestrator. A non-Claude/Codex harness normally delegates a repository
task to one native CodeFlow host rather than wrapping a second nested duo.

Claude hosts use the primary Fable-class seat at high by default, may route
bounded operations to Opus medium/high, and call the primary Codex coding seat
at high. Codex hosts use the primary Sol-class seat at high by default, may use
verified native Sol medium/high or qualified Terra-class workers, and call the
primary Fable seat at high. Existing xhigh triggers remain. Fable owns all
Claude-side worker routing. Missing or unverified routes degrade explicitly;
internal workers never replace primary planning or approval duties.

The contract lives in an on-demand capability-routing resource beside the
existing quality resource. It is behavioral doctrine and evaluator data, not a
new engine policy or general runtime plugin system.

## Rejected options

1. **Keep the fixed Codex producer.** This is simple and structurally
   independent, but ignores task-specific tools/context and makes host-aware
   delegation largely cosmetic.
2. **Put all routing prose in the orchestrator skill.** This avoids one file but
   lengthens an already dense skill and invites assignment/evidence drift across
   harnesses. A shared progressive-disclosure resource is the established
   pattern.
3. **Build a generic engine router or model auction.** Deterministic enforcement
   is attractive, but most assignment inputs require situated judgment and
   runtime evidence. No observed need justifies a new schema, dependency, or
   plugin boundary yet.

## Consequences

- Either primary lineage may produce a suitable task, but every material unit
  retains an independent other-lineage review and integrated Fable judgment.
- Assignment and reassignment become explicit, reviewable parts of Plan vN.
- Usage can influence routing only when observed; absent telemetry cannot become
  a quota claim.
- Host/peer/worker roles prevent recursive orchestration when Hermes or another
  persistent runtime delegates development to CodeFlow.
- Adding a future model or harness requires qualified native interactive
  transport, tool access, role isolation, evidence, and review independence;
  CodeFlow does not need an engine rewrite merely to name another vendor.

## Architecture impact

The scaffold gains a managed capability-routing resource, updated skill/docs,
and deterministic/model-eval pins. No Rust runtime routing or pipeline schema is
added. CAP-010 remains the capability record for the duo.

## Note (2026-09-25)

Seats: the producer and reviewer seats in this decision are catalog seats
(ADR-0069), resolved per duty. `unit-review` owes the lineage opposite the
actual author, and `body-review` owes both standing seats. The Fable-class and
Sol-class wording describes the roster at the time.

Approval: ADR-0070 amends the clause that a changed producer or reviewer seat
needs fresh approval from both primary seats, in two cases. A reversible item
settled after two rounds carries `SETTLED_DISSENT` in place of the dissenting
seat's approval. After a recorded mid-run seat loss, every available standing
seat approves the reassignment, the lost seat is recorded unavailable with
reduced assurance, and any verdict it gave before the loss stays as given.
Every other part of this decision stands.
