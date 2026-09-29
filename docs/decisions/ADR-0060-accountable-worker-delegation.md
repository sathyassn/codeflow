---
id: ADR-0060
uid: 3e7538f2-80c6-4087-9857-f91fbd0b760b
title: separate primary accountability from worker execution
date: 2026-09-11
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — plans record accountable primaries separately from evidenced execution routes
---

# ADR-0060: separate primary accountability from worker execution

## Context

CodeFlow's primary seats own planning, judgment, integration and acceptance,
but earlier guidance often used “producer” for both that responsibility and the
session that performed a bounded implementation unit. That ambiguity left
typed worker routes descriptive and encouraged capable routine work to remain
on the primary even when delegation would preserve its later judgment capacity.
Schema v4 now distinguishes candidate routes from scoped-qualified routes
without changing primary binding or project-selection formats.

## Decision

Plan vN records the responsible primary separately from execution mode, exact
binding or route plus effort, routing reason, requested-versus-observed native
provenance and available usage evidence with freshness or an explicitly unknown
value. A candidate route may perform bounded
non-design work after its native route is proven for the current harness; the
primary inspects, integrates and accepts the result. Candidate means the route
has no scoped quality qualification—it does not mean configured, available or
applied. Substantial well-specified routine work uses a capable permitted route
when available; direct primary execution stays valid for a tiny warm-context
change, inseparable unresolved reasoning, material risk, unavailable route or
bounded recovery, with a concrete reason for substantial retention.

Design direction and real design implementation remain under the Claude design
owner. Its primary executes while no matching Claude route is scoped-qualified;
a candidate design route is usable only in controlled disposable qualification
fixtures. A scoped-qualified Claude route may execute only the settled tuples it
evidences without acquiring direction or fidelity-approval authority. The
ensemble's recorded same-Claude primary fallback retains Claude authority after
native preflight when the preferred primary is unavailable.
Another family may design only when the operator explicitly names that task
override in Plan vN; Claude absence alone is not an override.

Native usage evidence records its source, observation time, harness and
account/bucket scope, shared-bucket relationships, remaining/reset values when
exposed, and freshness. Unknown telemetry stays advisory unless the brief makes
a hard limit depend on it. Same-family workers or an approved cross-family
primary may execute, but the receiving primary owns its workers and actual
authored lineage determines independent review. No scheduler, task-ledger
policy engine, worker registry, automatic promotion or economy claim is added.

## Consequences

Routine execution can preserve primary context while keeping responsibility
and every quality, review, security and integration gate intact. Records become
more explicit, and retained-primary work sometimes needs a short reason. Native
route availability and application must be observed independently of candidate
or scoped-qualified status. A scoped workload qualification remains narrower
than full primary-binding promotion, and measured all-attempt benefit remains a
separate prerequisite for any economical-default recommendation.

## Architecture impact

The scaffold contract and evaluation corpus now distinguish responsible
primaries from actual execution routes and provenance. The binary remains a
validator and diagnostic surface; routing decisions and evidence stay in the
orchestrated Plan vN and reviewed task artifacts.

## Note (2026-09-25)

The design owner is seat `claude-primary` on its first line (ADR-0069). The
sentence on the recorded same-Claude primary fallback is narrowed: the seat's
later line may hold the seat for orchestration, planning and review with
reduced assurance, but it does not author design or give design or fidelity
approval, and the `design` duty stays open, unless an `OPERATOR_OVERRIDE` block
in the approved Plan vN task record names that exact task, duty, route and
effort. Workers never hold direction or fidelity approval. Candidate and
scoped-qualified routes are now catalog versions with scoped evidence for one
exact tuple.
