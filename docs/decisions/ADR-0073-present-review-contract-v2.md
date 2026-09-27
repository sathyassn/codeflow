---
id: ADR-0073
title: Present review contract v2
date: 2026-09-26
status: proposed
superseded_by: null
architecture_impact: "docs/architecture/present.md: live html stages are inlined, entity anchors are checked by the service, and answers join the private session ledger"
---

# ADR-0073: Present review contract v2

Number reserved at planning time (EPC-016 Plan v5.1) and rechecked free by
TSK-117 before this record was written (closeout of TSK-117).

## Context

A real review of a cf-present document found three failures: a comment
could not target a node or arrow inside a figure (the picker climbed to the
enclosing `svg`), figures carried no visible title or explanation, and no
form could submit an answer to the agent. The same content as a Claude
artifact did better on the last two. The operator asked that cf-present
match or exceed the artifact system in every offering and, on 2026-09-26,
decided the scope (workspace `design/present-parity/proposal-present-parity-v2.md`,
v2.1, section 5). Literal parity would turn agent-authored pages into
applications with scripts, a shared database, connectors and remote
identity, which ADR-0049 keeps out of the runtime boundary.

## Decision

Present review moves to contract v2, specified in SPC-014. Forms are owned
by the runtime: a `form` block rendered and validated by present, submitted
through the service with a receipt, and stored as append-only answers bound
to the revision shown; authored HTML stays script-free and still cannot
submit anything. Review targets inside figures are named entities (figure
declaration ids and a closed `data-cf-*` vocabulary in `html` stages) that
the service validates, and notes re-anchor across revisions by entity,
quote, block and document. Figures and tables are framed by the runtime
with a visible numbered title, caption, legend and one Details disclosure.
The scope matches or exceeds every artifact offering that serves review and
does not copy the application platform: no shared database, rooms,
page-side model calls, connectors, hosted identity or permission grants
(decision 1a). There is no remote or phone access now (decision 3a).
Answers, notes and replies stay in the private local session store on the
machine and are never committed; accepted decisions are promoted to
records through the normal workflow (decision 4a). An answer reaches the
agent as untrusted evidence of the operator's choice, never as authority to
bypass a gate.

Relation to other records: this refines ADR-0049 (the present runtime
boundary) without widening it. The loopback service, owner-only state,
single-use bootstrap and script-free authored content stand; the new page
routes sit behind the same request checks. It extends ADR-0068 (figure
grammar as the default form) with required framing and entity ids, and the
numbering and legend rules land in the shared `figure-grammar.md`.

## Consequences

- A reviewer can comment on one node, arrow or label and the note survives a
  regenerated figure; a forged or mislabelled anchor is refused by the
  service rather than trusted from the page.
- Agents get typed, acknowledged answers and can wait for them with
  `present feedback --wait`, woken per event, instead of polling until the
  session closes.
- The v1 surfaces stay: v1 documents, the v1 feedback stream and 3.0.0
  sessions. Schema v2 documents cannot be opened by a 3.0.0 binary, and
  sessions carrying an entity note or v2 revision data cannot be read by
  one.
- Present gains a second append-only ledger (`responses.jsonl`) and more
  service routes to keep bounded and tested.
- Ruled out without a new decision: scripts in pages, remote viewers,
  multi-user identity, committing answers to the repository.

## Architecture impact

`docs/architecture/present.md`: the session surface row for untrusted static
HTML describes the inlined, validated stage in the live session and the
sandboxed iframe only in export (corrected by TSK-117); the entity anchor
check, the answer ledger and the new routes are added by the tasks that
build them (TSK-118 to TSK-121).
