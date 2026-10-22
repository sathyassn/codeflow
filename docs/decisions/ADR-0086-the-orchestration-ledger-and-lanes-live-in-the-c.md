---
id: ADR-0086
uid: 6be2c341-340f-4add-9d89-ecd41ae5994b              # hidden record identity, written once by `new`; never edit
title: The orchestration ledger and lanes live in the clone
date: 2026-10-22
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md gains one line, the clone's git directory holds CodeFlow's coordination working state (codeflow/coord/) beside its ledger, locks and registry state"
---

# ADR-0086: The orchestration ledger and lanes live in the clone

## Context

An orchestrating session owns a ledger of open commitments that
CodeFlow gives no shape, no closure rule and no home (issue 272). In an
adopting project a hand-written ledger stopped after nine days, items
the operator handed over went quiet, a merge of lane stocktakes gave
about 940 rows mostly duplicating tracked work, and a deleted lane
session lost the seats and reviews it was running. The native task
tools each harness offers (`cf-model-orchestrator/resources/task-graph.md`
names them as the planning aid) are session-scoped and vendor-specific,
so they cannot hold state that must outlive a session and be read by
any orchestrating model. The operator shaped the answer on 2026-10-22,
an independent Codex review of the design (sound with changes, 22
findings, 14 high) settled its contracts, and the operator decided the
points below; one choice stays open.

## Decision

The coordination working state lives in the clone, owned by CodeFlow in
schema, CLI, hooks and rules, and by the project in its entries. The
system is the coordination ledger (decided; "work ledger", "session
tracker" and "agentic tracker" were weighed and point readers at the
wrong subject). Its parts are follow-ups (`FU-NNN`, `codeflow follow`)
and lanes (`LN-NNN`, `codeflow lane`); the lead role is the
coordinator, to which the skills' existing `host` wording maps.

Location, three options were weighed. A: clone-local, in the common git
directory (`.git/codeflow/coord/`, found through Git's own common-dir
discovery), where CodeFlow's ledger JSONL, locks and registry state
already live; shared by every worktree of the clone; untracked; never
in history. B: a tracked file under `project-management/`, which needs a
pull request per change and is too heavy at minute-level churn. C: a
git-ignored folder in the root checkout, readable as a plain file but
bound to one checkout and lost with it. Decided: A, with
`codeflow follow list` as the table or Markdown view and an opt-in
redacted snapshot that is never committed automatically. The supported
live scope is one clone on one machine; the ledger carries an instance
UUID and FU and LN numbers are local display ids within it; an exported
reference is qualified by the UUID, and import is recovery into an empty
instance or an explicit reconciliation, never a live lease.

Format and transactions: one append-only JSON Lines log, `events.jsonl`,
holding follow-up, lane and coordinator events in one sequence, so a
hand-over and an adoption can never be read in the wrong order and one
append is one commit; liveness in small overwritten `status/` files,
never logged; `counters.json` reserved ahead of use, gaps allowed,
never lowered; `coord.lock`, a stable sidecar lock never removed or
replaced; `instance.json`. Sync before success is reported; retried
operations carry an idempotency key; hooks wait a bounded time and
report busy as distinct from empty; readers use the committed prefix;
network and model work run outside the lock and revalidate on commit; a
torn tail is repaired or quarantined before the next append; interior
damage, a duplicate or decreasing sequence, a broken transition or an
unknown state-changing schema version makes the ledger read-only with a
diagnostic; events are validated on replay as on input. No compaction
in the first cut: the 10,000-event read path is measured first, and if
needed later compaction is a snapshot-plus-tail generation, never
monthly archiving. Every event carries the envelope, field names in
full: `schema_version`, `event`, `subject_id`, `sequence`,
`recorded_at`, `recorded_by` (role and actor instance id; harness,
model and session when known) and optional `comment`; a mutation that
depends on ownership carries the expected generation.

Follow-ups: `FU-NNN`, a full record for a cold reader (`title`,
`description`, `done_when`, `source` and its reference, `holder`,
`links` typed context, prerequisite or fulfils, or a `no_link_reason`,
an effective schedule; `awaiting_operator` is a condition, not a
holder). Events: `item_added`, `item_updated`, `item_checked`,
`item_handed_over`, `item_closed`, `item_dropped`, `item_reopened`. A
prerequisite changing state makes a follow-up due; only a declared
fulfils condition whose evidence satisfies `done_when` on the target,
judged by the existing target-aware readiness and acceptance machinery,
closes it; completion on a feature branch never counts; a reverted
proof reopens with the old proof kept. Evidence is marked asserted or
machine-verified with a reference a fresh session can open. One
permission matrix covers add, update, check, hand-over, close, drop and
adopt; agents never record as the operator. Only exact duplicates (same
source identity or idempotency key) are refused; similar ones are
suggested. Every free-text field is scanned and bounded, the files are
private, an explicit purge path exists.

Lanes: a session is a process; the coordinator is the lead role; a lane
is a durable, harness-neutral unit of delegated work; a seat is the
harness session attached to it, replaceable, with a recorded kind
(external, resumable, in-process); an attempt is one run of work with
its own task branch and worktree, recording task, worktree, branch,
revision, expected result and native provenance. `LN-NNN` rises and is
never reused; the slug is presentation derived from a descriptive scope
(task, tasks, epic or topic) that changes as `lane_scope_changed`
events and never amends tracked records; readiness applies to starting
implementation through the same core as `work start`. Events:
`lane_opened`, `lane_scope_changed`, `seat_attached`, `seat_detached`,
`lane_reported`, `lane_adopted`, `lane_closed` (done with evidence and
a disposition for open follow-ups, abandoned, or superseded naming the
successor); `coordinator_claimed`, `coordinator_adopted`,
`coordinator_released`. The coordinator lease carries an actor instance
id and a generation; adoption compares the expected holder, generation
and observed heartbeat under the lock and increments the generation; a
stale writer is refused where the operation can check it. A lease is
coordination ownership, not an execution lock: before replacement work
starts, the old seat's live panes, in-flight operations, branches and
dirty worktrees are inventoried, anything not shown stopped is kept,
and nothing is cleaned up from a timeout. Only the coordinator opens
lanes. `work claim` keeps its behaviour (a hint and an optional
`--lane`); `cf-herdr` seat open takes `--lane` or a recorded `--no-lane`
reason and keeps its live identity checks. The recovery inventory
(dirty worktrees, unpushed and unreferenced commits) is in scope.

Timing: `check interval` (default 10 minutes) and `heartbeat timeout`
(default 30 minutes) are configurable advisory defaults, the timeout
longer than the interval because one long command refreshes no
heartbeat; every hook firing and every `codeflow` command refreshes the
caller's status file; expiry means contact overdue, never death;
takeover is only by explicit `adopt`, and a returning session that
finds its role or lane adopted stops and says so.

Switching harnesses and models: a new coordinator on any harness keeps
the same lane for the same work and attaches a new seat that continues
from disk; external seats carry on after Herdr's live checks; resumable
seats resume in their harness; in-process seats are lost and their
attempts inventoried; a lane closes as superseded only when its scope
changes, its state cannot be recovered or the operator restarts it;
review independence is judged against the work's author, not the
coordinator; native task lists stay private aids.

Enforcement is five layers, never a session timer, only the first of
which depends on the model remembering: the managed rule and skill; the
read-only session-start report; the bookkeeping commands; the turn-end
check, warning by default and never blocking inside a tracked delegate
session, blocking elsewhere only as a verified project opt-in; and
status built from `follow list`. One implementation serves every
harness through adapters, with a published capability matrix: Claude
wires start, stop and prompt; Codex wires start and prompt in
`.codex/hooks.json`; Grok has PreToolUse only, so it gains a session
hook where supported or runs `codeflow orient` explicitly. Observations
of pull requests and tasks are bounded, cached and offline-aware; no
daemon scheduling.

The ledger is a coordination aid, never a safety control: it never
blocks git, records, CI or landing; a missing or damaged ledger only
warns; coupling is one way (the ledger reads task, epic and ADR state
and never writes a record; records, pull request bodies and
`codeflow ci` never cite FU or LN ids and never read the ledger, which
is clone-local and absent in CI); an item that becomes real work
becomes a task through normal planning citing its real origin.

This amends the doctrine in `cf-model-orchestrator/resources/task-graph.md`:
the native harness task tools remain the per-session planning aid, and
the coordinator's running state lives in the ledger. ADR-0076 (one pull
request per task, planning once per epic) is unchanged.

Open, for the operator: the Codex hook route, established by TSK-440.

## Consequences

Easier: any session, model or seat reads one list; an interrupted lane
is recoverable from the clone with its attempts and inventory; status
replies are built from recorded follow-ups, not memory; operator asks
have an owner and a next check that survive a session; a coordinator
can change harness or model without losing lanes. Harder: every
session binds an actor instance and claims a role explicitly; adoption
needs an inventory before replacement work; the guards must keep
allowing CodeFlow's writes under `.git/codeflow/coord/` while refusing
direct edits; a project with several clones has several ledgers, and
the snapshot is the only way to share one; the read path must stay
bounded without compaction. Ruled out: a tracked ledger in history, a
per-machine or cross-machine ledger, closure by hand-off, automatic
closure from a context link, automatic coordinator claims, semantic
duplicate refusal and any claim that a lease fences arbitrary tools.
The guidance homes (the managed block, cf-model-orchestrator and its
task-graph doctrine, cf-herdr, cf-delegate, cf-method, the per-harness
notes, docs/architecture.md and the CLI help) change in EPC-025.

## Architecture impact

`docs/architecture.md` gains one line: the clone's git directory holds
CodeFlow's coordination working state (`codeflow/coord/`, one event log
with follow-ups, lanes and coordinator events, plus liveness files)
beside its ledger, locks and registry state; this is written in EPC-025
(TSK-423), since this ADR is proposed in a planning amendment and
accepted by the operator.
