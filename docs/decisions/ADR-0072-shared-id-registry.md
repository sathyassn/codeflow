---
id: ADR-0072
uid: 091c7334-d4ec-489c-b030-7c110906faae
title: Shared id registry on a protected data branch
date: 2026-09-26
status: accepted
superseded_by: null
architecture_impact: "`docs/architecture.md`: durable work gains a shared id registry on the `codeflow/registry` data branch, a hidden `uid` per record bound to its number before merge, history-based allocation with typed restore, and the advisory claim branch as the visible mark of work in progress. Updated in the PR that accepts this record (TSK-101)."
---

# ADR-0072: Shared id registry on a protected data branch

## Context

ADR-0046 gave epics, specs and tasks independent repository-wide sequences
and warned that sequential allocation from one checkout can collide across
worktrees that cannot see each other's records. It left the remedy to
planning discipline: serialise allocation or renumber before merge. That
remedy did not hold once several agents planned on several integration
lines. `task new` on main offers TSK-070 and EPC-018, both of which already
exist on other lines; TSK-079 was issued twice and renamed by hand before
it landed. The audit that opened this work found the same pattern behind
stale statuses and hand-edited records: the number a record carries is
decided by whichever checkout ran the command.

An id is written into commits, PR titles, branch names and links the moment
it is issued, and history is never rewritten. So the number must be unique
when it is issued, or at the latest before the record merges, and no later
repair may rename what has landed. The remedy also has to work for every
adopter, offline, on forks, and on hosts whose free plans offer few or no
branch rules. Reviews of the design by three seats in two rounds
established one more constraint: a host's branch protection guards a
branch's history and refs, not the files inside a commit, so a design that
relies on the host to keep a file from being deleted is wrong.

## Decision

Every repository that tracks work has one shared id registry: a data
branch named `codeflow/registry` on the authority remote, holding one small
file per issued id under `ids/<KIND>/<N>.toml`. Issuing an id is a plain
push of one new file on top of the fetched tip. The host accepts the push
only if it extends the current tip, so the branch tip is the
compare-and-swap that serialises issuers; a rejected push means fetch,
recompute and
retry. Every record also carries a hidden random `uid`, written once by
`new`, and every record added in a PR must be bound to its number in the
registry's history before the PR can merge. Once a number is bound to a
`uid` in an append-only history, no later issuer can take it.

The branch carries a data profile, applied by `codeflow remote protect`
separately from the rules of `main`: no branch deletion and no force push,
no pull-request requirement, no branch naming and no product test gate.
The design trusts that protection only as far as it goes. The next number
is computed from every id ever added in the registry's history and on every
local and remote-tracking ref, so deleting a file on the tip cannot lower
it. Append-only is enforced before the push by CodeFlow's pre-push hook and
git-guard over the whole pushed range, checked by the CLI on the fetched
history before every issue, and checked by `codeflow ids check` in the
code branch's CI on every PR and push and on a daily schedule. The data
branch holds no workflow or code, so nothing runs from it and a PR cannot
choose the binary that checks it. Damage refuses issuance until a typed
restore repairs it: one maintainer commit that returns the named files to
the bytes of their first addition and nothing else, which the same guards
accept and any other edit fails. On a host without branch rules, the CLI
keeps the last verified tip and refuses to issue when the new tip does not
descend from it, and `doctor` reports reduced assurance.

Offline issue commits a pending reservation on the local
`codeflow/registry`, kept apart from the remote-tracking copy by a
non-forced fetch, and `ids sync` replays it on the authority before any
online write. A clash on an unmerged record is renumbered by
`ids retarget`, which records the old id. Fork and hand-written records are
admitted by a maintainer with `ids admit`, which reserves the number for
that record's `uid`; untrusted PR code never receives write credentials to
the registry. The registry is seeded once from every id on every ref and in
retained history, giving each existing record a `uid` that a backfill then
writes into the record on each line, so no legacy exemption survives.

Two settled rules travel with this decision. Claims are advisory in 3.0.0:
`work claim` pushes a visible task branch and conflicts are reported; no
lease service exists and no document calls the branch a lock. Small fixes
may land as a direct change with `Task: none: <reason>`, except on the
protected surfaces (watched contract paths, policy, hooks, dependencies,
the record schema, adopter-facing surfaces), where a task is required.

The behavioural contract, including the issue protocol's outcome
classification, the merge rule, sync, retarget, admit, seed and backfill,
is SPC-013.

## Consequences

- Ids stay readable and sequential. Filenames, branch names, commit
  subjects and links keep the grammar people already use; no habit
  changes and no history is rewritten.
- Uniqueness is honest. Online, a number is unique at issue. Offline, it is
  "pending, not unique yet" until sync. In every case a duplicate is
  detected before merge by the `uid` binding, never after landing.
- The host is trusted for what it protects: the branch's history and refs.
  File-level integrity is CodeFlow's own work, on three planes, and is
  made harmless by history-based allocation even when a plane is bypassed.
  Damage never reissues a number, because the bindings stay in history;
  its real cost is availability: one damaging commit can remove any number
  of files, and issue stops repository-wide until a maintainer's typed
  restore lands. The registry check is not a job on the data branch; it
  runs at the next CI run of any tracked branch and daily, so detection
  is bounded by that cadence, while the CLI's own check before each issue
  is immediate for the issuer.
- The saved last-verified tip detects a rewrite only for an observer that
  holds that checkpoint. A fresh clone on a host without branch rules has
  no such proof and reports unknown assurance rather than claiming
  prevention.
- The registry is one more branch in every tracking repository. Clones
  fetch it explicitly, CI fetches it with full history, and maintainers
  create and seed it once. Contention is on one tip, so a busy moment
  costs a retry; the cost is measured in the benchmark, not assumed.
- Forks and hand-written records gain a maintainer step (`ids admit`) and
  stay red until it runs. An abandoned admission leaves a gap, which is
  acceptable; a reissued number is not.
- Exclusive claims are not provided. A second orchestrator competing for
  the same backlog would need the lease design planned on the follow-on
  epic; until then one orchestrator assigns disjoint work.
- Two identities per record is a dual-identity design. The `uid` is
  hidden, costs a reader nothing, and is an integrity check only; number
  coordination remains, and the `uid` proves nothing about a record's
  meaning.
- The decision is gated. TSK-100 proves the data-branch profile, seeding,
  issue, lost acknowledgement, deletion caught by history allocation and
  by the registry job, admission of a fork-style record, and whether the
  host offers a file-path push rule, on a real host in a disposable
  repository before TSK-101 builds the registry. If TSK-100 fails, the
  epic stops and the operator's decision returns. The fallback for 3.0.0
  is option 1b below: allocation from every visible ref plus the CI
  binding check, with clashes renumbered before merge. It is knowingly
  weaker (no uniqueness at issue) and would be recorded as a superseding
  Note here, not presented as equivalent.
- Every registry rule is on the never-adjustable list once tracking is on.
  Adopters cannot turn it off, and CodeFlow's own repository runs the same
  registry as any adopter's.

## Alternatives considered

### Scan of every ref plus a CI check (option 1b)

Allocate from the highest id visible on any local or remote-tracking ref
and let CI refuse a PR whose new record's number already exists on the
target or another line. Smallest to build and no new branch. Rejected as
the primary design because two clones that cannot see each other still
take the same number, and the repair is a renumber before merge, which is
the manual step this work exists to remove. It remains the 3.0.0 fallback
if the host proof fails.

### Random ids only (option 1c)

Hash or UUID ids need no coordination and never collide in practice.
Rejected because every id in commits, PR titles, branch names and links
would change grammar, a migration would touch every record and every
habit, and long ids are hard to read and to say. The evidence from beads
(below) also shows that any sequential sub-counter reintroduces the
collision, so the change buys less than it costs. The random id is kept as
the hidden `uid` for integrity, which takes the benefit without the cost.

### Custom refs instead of a branch (design v2)

One ref per id under `refs/codeflow/ids/` gives the same compare-and-swap
per ref. Rejected because GitHub cannot protect custom refs, every clone
and workflow needs an extra refspec, pending local reservations have no
natural home apart from the remote copy, and the history is not readable.
A branch push is the same compare-and-swap on one tip, adds host protection
of history, a plain fetch, a separate tracking ref for pending state, and
history people can read. The design seat that argued for refs withdrew the
position in round 2.

### A registry file on main, changed by PR

Puts the registry under full branch protection and review. Rejected because
every id would wait for a merge, which is slower than the collision it
prevents, and the file would conflict on every parallel line.

### A server-side writer or required pre-admission check

A restricted writer in front of every registry push would prevent file
deletion rather than detect it. Rejected because it is a service, which
this design avoids, and because history-based allocation already makes a
deleted file harmless. TSK-100 checks whether the host's push rulesets can
restrict paths on the registry branch, which would add prevention for
free where offered.

### A database or service

Rejected. Records must stay readable without a database, land with the code
in the same PR, and work offline; the prior art shows what leaving git
files for a database costs.

## Prior art

Three systems shaped this decision; the research report under the
workspace's `design/delivery-system/` holds the sources.

- beads (gastownhall/beads) tried the three identity models in order:
  sequential ids, then sequential ids with automatic renumbering on merge,
  then random hash ids, each step forced by collisions or silent data loss.
  It moved its store from JSONL in git through a custom merge driver and a
  sync branch to a separate database, and its own docs say that once the
  store left the code branch, issue state and code state diverged and
  gates were needed to reconnect them. Its per-parent sequential child ids
  reintroduced the original collision. Lessons taken: allocate from shared
  history, never from local state; never renumber automatically after
  landing; keep records in the tree so they land with the code; keep
  multi-clone collision tests.
- git-bug stores each entity as an append-only chain of operations under a
  custom ref, with the id as the hash of the first operation. Lesson taken:
  append-only operations remove state conflicts and hash-of-creation ids
  remove id collisions. Cost observed: custom refs need their own push and
  fetch and are invisible in normal review, which is why this decision uses
  a branch and keeps the readable id.
- git-appraise keeps reviews in git notes, one JSON object per line, merged
  by the notes union strategy with the reader deciding current state.
  Lesson taken: one file per event, never edited, merges without conflict
  and lets the reader compute the current state. The registry's one file
  per id and history-based allocation follow that shape.

## What this supersedes

ADR-0045 is already superseded by ADR-0046. ADR-0046 stays accepted for the
independent id grammar, one persisted `id` equal to the filename,
frontmatter relationships, one epic or a standalone rationale per task, the
planning anchor, and the rule that a task branch cannot authorise its own
planning record. This record changes three of its clauses and no other:

| ADR-0046 clause | Change |
|---|---|
| "Planning therefore serializes allocation or renumbers collisions before merge." | Superseded. Allocation is the registry protocol; a renumber before merge (`ids retarget`) is the exception path for an offline clash, never the rule. |
| `work start` requires "completed dependencies". | Amended. A code dependency must be complete and present in the task's execution base; a research or decision dependency is satisfied at a pinned commit; the same core predicate serves a new claim, the caller's own task branch and a PR transition, with the caller context stated (SPC-013 R-40, R-110, R-112). `work start` stays read-only. |
| "No surface creates records, branches, worktrees, or status changes." | Amended. `work claim` creates and pushes the task branch as the visible advisory mark; `task status`, `epic status` and `spec status` write status under transition rules; `ids retarget` renumbers an unmerged record. No surface creates a worktree, and `work start` still writes nothing. |

Two clauses that a reader might expect to change do not. "Specs have no
duplicate parent relationship" stands: consumers own their `specs` lists
and a spec's consumers are derived, so many-to-many consumption adds no
reciprocal list. "New records persist one `id`, equal to their flat
filename" stands: `former_ids` and `uid` are additions beside it.

The marking is a dated Note on ADR-0046 pointing here, written in the PR
that accepts this record, since the rest of that record is unchanged and
its `superseded_by` field would imply the whole decision fell. ADR-0062 is
unchanged: the registry branch is outside release state and the
calculator.

## Architecture impact

`docs/architecture.md`: durable work gains a shared id registry on the
`codeflow/registry` data branch, a hidden `uid` per record bound to its
number before merge, history-based allocation with typed restore, and the
advisory claim branch as the visible mark of work in progress. The PR that
lands the registry (TSK-101) set this record accepted, wrote the ADR-0046
Note and updated `docs/architecture.md`.

Accepted 2026-09-26 with one amendment from the TSK-100 host proof: the
enforcing registry check runs on `pull_request_target`, so its workflow comes
from the target branch and never from the pull request it judges (SPC-013
R-109).
