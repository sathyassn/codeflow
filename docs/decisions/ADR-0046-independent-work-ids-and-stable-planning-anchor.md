---
id: ADR-0046
uid: 04d829d4-1017-4f0a-90e9-b2107482636d
title: use independent work ids and a stable planning anchor
date: 2026-07-29
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — durable work gains independent IDs, explicit relationships, and a shared merge-base start preflight
---

# ADR-0046 — independent work IDs and stable planning anchor

## Context

ADR-0045 established Git Markdown as durable work authority and one flat
layout, but encoded the parent epic in task IDs and left specs outside CLI
allocation and graph enforcement. A task could also be created only on its
implementation branch, so neither review nor automation could prove that
planning had first reached the intended integration target.

The durable workgraph must remain understandable without a database, support
standalone tasks without fake parents, preserve historical repositories, and
give interactive and CI workflows the same non-mutating readiness answer.

## Decision

New epics, specs, and tasks use independent repository-wide sequences:
`EPC-NNN`, `SPC-NNN`, and `TSK-NNN`. New records persist one `id`, equal to
their flat filename. CodeFlow continues to read historical dual-identity,
`TSK-NNN-NNN`, and nested records.

Relationships live in frontmatter. A task references one epic or records a
non-empty standalone rationale, lists direct predecessors, and names its
non-task integration target. A task branch is an implementation surface and
cannot authorize its own planning record. An epic or task lists consumed specs;
a task inherits its epic's specs. Specs have no duplicate parent relationship.

Planning records are allocated and validated on a planning branch and merged
into the declared target before implementation. `codeflow work start` inspects
the task branch and the merge-base with that target, requiring the task,
matching target declaration, parent/rationale, approved specs, and completed
dependencies. It accepts only a real local or remote-tracking non-task branch,
rejecting task branches, `HEAD`, tags, object IDs, and revision expressions,
and is read-only.
Task allocation requires that target branch to exist already; record titles are
stored as YAML-safe, non-empty single-line labels. This keeps malformed records
and imaginary integration anchors out of the shared workgraph.
The CLI, task-branch pre-commit hook, and task-branch CI first validate the
visible workgraph and then apply the same stable-anchor rule. Planning belongs
on a `plan/` branch: a task branch cannot commit planning records that authorize
its own implementation. CI applies the rule with its explicit branch identity,
including detached checkouts. No surface creates records, branches, worktrees,
or status changes.

## Consequences

IDs no longer imply mutable hierarchy, standalone work is explicit, and specs
are first-class graph inputs. Git history itself supplies the planning seal, so
there is no digest file or second authority to synchronize. The same core rule
produces early local feedback and authoritative CI evidence.

Sequential allocation can collide across worktrees that cannot see each
other's uncommitted records. Planning therefore serializes allocation or
renumbers collisions before merge. Historical records remain readable; only
new canonical tasks require an explicit integration target.

## Architecture impact

`docs/architecture.md` now describes independent IDs, frontmatter
relationships, legacy compatibility, and the shared merge-base start preflight.

## Note, 2026-09-26: ADR-0072 changes three clauses

ADR-0072 (shared id registry) is accepted. It changes these clauses of this
record and no other; everything else here stays accepted.

- "Planning therefore serializes allocation or renumbers collisions before
  merge." Superseded: ids are issued from the `codeflow/registry` data
  branch by compare-and-swap push, and `codeflow ids retarget` is the
  exception path for an offline clash, never the rule.
- `work start` requires "completed dependencies". Amended: the dependency
  kinds and the shared readiness predicate of SPC-013 (R-40, R-110, R-112)
  apply; `work start` stays read-only.
- "No surface creates records, branches, worktrees, or status changes."
  Amended: the status verbs write status under transition rules,
  `work claim` pushes the advisory task branch, and `ids retarget`
  renumbers an unmerged record. No surface creates a worktree.

"New records persist one `id`, equal to their flat filename" stands: `uid`
and `former_ids` are additions beside it.

## Note, 2026-09-29: ADR-0076 narrows one clause

ADR-0076 (one PR per task and planning once per epic) is accepted. It
changes one clause of this record and no other; everything else here stays
accepted, the planning anchor included.

- "Planning belongs on a `plan/` branch: a task branch cannot commit
  planning records that authorize its own implementation." Narrowed to
  epic tasks. A standalone task's own single record may be committed on
  its task branch and lands with its code in one reviewed PR, read at head
  (SPC-013 R-78, R-110); every other record in that PR stays refused.
