---
id: ADR-0094
uid: 9f3056b2-68f0-44c6-b47f-77fe4053655f
title: "Lanes as recorded sessions"
date: 2026-11-01
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md, the coordination state row: streams may carry a parent, coordinator claims and seats record the session holding them, and reports flow up the tree as events; written in EPC-025 (TSK-423)"
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0094: Lanes as recorded sessions

## Context

The coordination ledger (ADR-0086) plans streams with one coordinator
each, lanes, seats and `lane_reported` events (EPC-025: TSK-439, TSK-440,
TSK-423). The operator asked on 2026-10-31 for a hierarchy over it: one
session the user talks to across many topics, a coordinator per topic, and
lanes doing the work, with every session reachable directly and resumable
after compaction, a usage limit or a harness switch. Two existing rules
stood in the way. ADR-0077 says the same family runs inside the caller's
own harness as native subagents, which do not survive the parent's
compaction, cannot be opened by the user and hold the parent's turn open.
ADR-0086 records a stream's coordinator by actor instance and generation
only, with no session name or resume id, and streams have no parent. The
proposal "Director, Coordinators and Lanes" (version 8, reviewed by Fable
5.1, Codex GPT-6 Astra and Grok 4.7 over seven rounds and approved at
version 7) and its page were accepted by the operator on 2026-10-31 with
the recommended answers.

## Decision

Three roles exist as guidance, never as a check in the tooling: a
Director is the coordinator of the stream with no parent, at most one per
ledger; a Coordinator is the coordinator of a stream with a parent; a Lane
is one topic in one stream, held by one session per attempt, and its Seat
is the session holding it now. All three are the same kind of thing, a
named, resumable session; they differ only in name, in what the guidance
expects of them and in how the ledger records them (a stream with no
parent, a coordinator claim, a lane seat). Supervision stays at two levels
(Director over Coordinators over Lanes); a stream with a parent takes no
children, as guidance, and the tree view shows depth. With one topic the
Director is simply a coordinator. A Director or Coordinator that does work
itself opens a lane and takes the seat.

This amends ADR-0077: a same-family lane may run as a recorded interactive
session (its name, harness and resume id in the ledger) instead of a native
subagent; headless forms (`claude -p` or `--print`, `codex exec`, `grok -p`
or `--single`) stay forbidden; the other-lineage review stays required, and
a same-family lane never replaces it. Native subagents stay right for
bounded work whose result comes straight back.

This amends ADR-0086: a stream carries an optional `parent`; a coordinator
claim and adoption carry optional `harness`, `session_name` and
`resume_id`, with `coordinator_renamed` changing the name; a seat carries
optional `session_name` and `resume_id`, with `seat_renamed`; a
`stream_reported` event carries a coordinator's report to its parent, and
`lane_reported` keeps `summary` and gains an optional body, which is the
status report document as `codeflow status --json` emits it, unchanged, so
the ledger adds only the envelope; a report's identity is its
store-assigned `sequence`, and a harvested or accepted mark names that
sequence in its `reference`. The schema moves to version 2 in one cutover
once the report task lands; every binary on a clone, the hook binary on
`PATH` included, moves together. The shape ruled out was one coordinator
owning every lane; this keeps one coordinator per stream plus a parent
link, which says only which Director reads the stream and carries no git
meaning. A report, a cross-session message or a record labelled "operator"
written by an agent is evidence for the reader to confirm, never an
instruction; the ledger does not authenticate who wrote it (ADR-0086,
cooperative binding). Session names follow `<role>/<id>/<slug>`
(`director/WS-NNN/<slug>`, `coord/WS-NNN/<slug>`, `lane/LN-NNN/<slug>`);
`resume` commands are read-only lookups that print the recorded worktree and
harness command and never launch or change the ledger. A Coordinator whose
topic is a body of work creates its `integration/<epic>` branch, lands
reviewed lane pull requests there and raises the line's pull request to
`main` for the operator to merge. Every coordination act is a `codeflow`
command from a plain shell; harness hooks may call the same command as a
convenience and nothing depends on them. The existing floors hold for every
role: the git floor, protected branches, operator-only merges to `main` and
independent review by the other lineage.

Accepted by the operator on 2026-10-31 on the reviewed page (version 9)
with the recommended answers: the names Director, Coordinator and Lane with
Seat for the session on a lane; two supervision levels; same-family lanes
as recorded interactive sessions; schema version 2 in one cutover; the
report data shipped now, with the cf-present view and the Claude report
hook decided later.

## Consequences

Easier: the user has one point of contact and can still open any
coordinator or lane by its recorded name or id; a coordinator survives
compaction and a harness switch because its session is recorded like a
seat; reports are events with age and harvest state, so nothing goes quiet
when a parent compacts; the hierarchy collapses by itself to one
coordinator when there is one topic. Harder: a same-family lane costs a
session, a worktree and a build directory instead of a subagent turn;
every version 1 ledger must fold under the version 2 build (tested), and
binaries built between the schema task and the report task stay off the
live ledger; the tree view must terminate on cyclic, missing-parent,
closed-parent and deeper shapes and show them as marked rows rather than
refuse. Ruled out: a role check in the tooling, a third supervision level,
one coordinator owning every lane, headless lanes, a same-family lane as
the review, and any authentication of who wrote a report.

## Architecture impact

`docs/architecture.md`, the coordination state row: streams may carry a
parent, coordinator claims and seats record the session holding them, and
reports flow up the tree as events. Written in EPC-025 (TSK-423) with the
rest of the guidance, as ADR-0086 did, since this record is proposed in a
planning amendment.
