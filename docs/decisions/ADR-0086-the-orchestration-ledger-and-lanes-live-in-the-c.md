---
id: ADR-0086
uid: 6be2c341-340f-4add-9d89-ecd41ae5994b              # hidden record identity, written once by `new`; never edit
title: The coordination ledger and lanes live in the clone
date: 2026-10-22
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md gains one line, the clone's git directory holds CodeFlow's coordination working state (codeflow/follow/) beside its ledger, locks and registry state"
---

# ADR-0086: The coordination ledger and lanes live in the clone

## Context

An orchestrating session owns a ledger of open commitments that
CodeFlow gives no shape, no closure rule and no home (issue 272). In an
adopting project a hand-written ledger stopped after nine days, items
the operator handed over went quiet, a merge of lane stocktakes gave
about 940 items mostly duplicating tracked work, and a deleted lane
session lost the seats and reviews it was running. ADR-0076 plans work
in the harness's task tools, which are session-scoped and
vendor-specific, so they cannot hold state that must outlive a session
and be read by any orchestrating model. The operator shaped the answer
on 2026-10-22 and asked for the options and their costs on record; the
decisions below are the operator's, and three choices stay open.

## Decision

The coordination working state lives in the clone, owned by CodeFlow
in schema, CLI, hooks and rules, and by the project in its entries.

Location, three options were weighed. A: clone-local, in the common
git directory (`.git/codeflow/coord/`), where CodeFlow's ledger JSONL,
locks and registry state already live; shared by every worktree of the
clone; untracked; never in history. B: a tracked file under
`project-management/`, which needs a pull request per change and is
too heavy at minute-level churn. C: a git-ignored folder in the root
checkout, readable as a plain file but bound to one checkout and lost
with it. Decided: A, with `codeflow follow list` as the table or
Markdown view and an opt-in tracked snapshot for a project that wants
history. One ledger per project (per clone), not per machine or session, so any
orchestrating model or session reads the same list.

Name: the system is the coordination ledger (recommended, pending the
operator; "work ledger", "session tracker" and "agentic tracker" were
weighed and point readers at the wrong subject); its parts are
follow-up items (`FU-NNN`, `codeflow follow`) and lanes (`LN-NNN`,
`codeflow lane`); the lead role is the coordinator; the files live
under `.git/codeflow/coord/`.

Format and data model: three append-only JSON Lines logs, each with
one job, and a small set of overwritten status files. `items.jsonl`
says what must be followed up, `lanes.jsonl` who is doing delegated
work and where, `roles.jsonl` which session is the coordinator;
`status/` holds one overwritten heartbeat file per holder, never
logged, because every command refreshes it; `counters.json` (next FU
and LN numbers, written under the lock), `coord.lock` and `archive/`
(closed history, compacted by month) sit alongside. Nothing in a log is
edited in place; the current state is folded from the events in order;
Markdown is only the reading view (`codeflow follow list --md`),
because concurrent edits to one Markdown file collide and cannot be
validated. Every event line carries the envelope, field names in full:
`schema_version`, `event` (from the closed list for its log),
`subject_id`, `sequence` (rises by one per log under the lock; order is
by sequence, never by clock), `recorded_at` (UTC, for people),
`recorded_by` (role required; harness, model and session id when
known) and optional `comment`, scanned for secrets. The event names are
`item_added`, `item_updated`, `item_checked`, `item_handed_over`,
`item_closed`, `item_dropped` and `item_reopened` (for a reverted
proof); `lane_opened`, `lane_scope_changed`, `seat_attached`,
`seat_detached`, `lane_adopted` and `lane_closed`;
`coordinator_claimed`, `coordinator_adopted` and
`coordinator_released`. The logs refer to each other only by id, and a
session reads them at start in the order roles, items, lanes, status.

An aid, not a control: the ledger never blocks git, records, CI or
landing, and a missing or damaged ledger only warns. `work claim`
keeps its behaviour, printing a hint when no lane holds the task and
accepting an optional `--lane`; it does not open lanes. `cf-herdr`
seat open requires `--lane LN-NNN` or a recorded `--no-lane "<reason>"`
so an unlinked seat is never silent (consults, canaries, evaluations
and the operator's own seats use `--no-lane`). The turn-end check warns
by default; blocking is a project opt-in. Coupling is one way: the
ledger may name task, epic and ADR ids and reads their state for
auto-close and readiness, and never writes records; records never
depend on the ledger (tasks, epics, ADRs, pull request bodies and
`codeflow ci` never cite FU or LN ids and never read the ledger, which
is clone-local and absent in CI). When an item becomes real work, it
becomes a task through normal planning, citing the real origin (issue,
pull request or review), and the item links to the task.

Follow-up items: `FU-NNN`, rising and never reused. An item is a full
record that stands on its own for a cold reader: required `title`,
`description`, `done_when`, `source`, `holder`, and `links` or
`no_link_reason`; optional `source_reference`, `waits_on`,
`next_check`, `needs_operator` and `priority`. An item is open, waiting,
due, closed (proof of a listed kind: merged pull request, commit, task
completed, ruling, operator answer) or dropped (reason, by the holder
or the coordinator), and is reopened when its proof is reverted. An item waits on a task, a pull request, a lane, another
row, the operator, an outside event or a time, and CodeFlow derives
when it is due from those changing state; a merged pull request or a
completed task closes its linked items with that proof; items point at
tasks and never copy the task graph; a cycle among items is refused. Any session may add an item. A holder is a role,
`coordinator` or a lane id, never a session id; only the holder closes an item,
on proof (a merged pull request or commit, a recorded ruling, the
operator's answer); "handed to a lane" never closes one; an item linked
to a task closes with the task. An item that duplicates an open record is
refused until it is linked or the match is rejected.

Lanes: a session is a process; the coordinator is the lead role; a
lane is a durable unit of delegated work; a seat is the harness
session attached to a lane, replaceable. A lane has an id
`LN-NNN-<slug>`, allocated per project, monotonic, never reused; it
records kind (build, review, research, consult), a scope, model, the
current seat (harness, pane, resume id, worktree, head), a heartbeat
and its items. A lane often carries more than one task, sometimes a
whole epic or a body of work on a topic, so its scope has a kind
(task, tasks, epic or topic) and refs (task ids, an epic id, or a topic
phrase); `codeflow lane scope` changes it over time, each change an
event, so the lane's history shows which work it carried. The slug is
derived from the scope: lowercase, short, unique among open lanes and
renamable; the number is the identity and never repeats. Readiness
follows the kind: a task lane starts when its task is ready; a tasks
lane starts each task when it is ready; an epic lane takes tasks in
the epic's order as each becomes ready; a topic lane builds nothing
until it creates or links tasks. A replaced lane closes as superseded
and names its successor; it moves through open, active, idle, closed (done,
abandoned or superseded, with proof) and archived, and is never
deleted. `codeflow lane open|attach|close|list` drives it; Herdr tab
labels carry lane ids and `cf-herdr` seat open needs `--lane` or a
recorded `--no-lane` reason; a build lane for a task starts only when
`work next` reports the task ready; a standalone session is its own
coordinator. The coordinator role is one session's lease with a
heartbeat. Two settings in the project configuration, placed by the
policy placement rule: `check interval`, how often the coordinator surfaces
due items (default 10 minutes), and `heartbeat timeout`, how long
without a sign of life before a coordinator or seat counts as gone (default
30 minutes); the timeout must exceed the interval, because one long
command (a 25 to 45 minute full gate) refreshes no heartbeat while it
runs, and heartbeats are refreshed by every hook firing and every
`codeflow` command. Takeover is never automatic: the start hook reports
a stale coordinator or seat, a session runs an explicit `adopt`, and a
returning session that finds its role or lane adopted stops and says
so. Items of a
holder whose heartbeat has stopped show as orphaned at the next session
start and are adopted by the next session claiming that role. The coordinator
sweeps all items and owns operator asks; a lane sees and closes its own
and adds items for what it hands back; every session sees operator-owned
items.

Enforcement is five layers, and never a session timer; only the first
depends on the model remembering. One: the managed always-rule and the
orchestrator skill. Two: `codeflow hook session-orient` at session
start claims or reports the role and prints the items owed, overdue,
orphaned and waiting on the operator, plus lanes with stale seats.
Three: the commands that do the bookkeeping (`work claim` hints at a
missing lane and links one given with `--lane`, `cf-herdr` requires
`--lane` or a recorded `--no-lane` reason, a merged pull request or
completed task closes linked items with that proof, `follow add`
refuses duplicates). Four: a turn-end check that reports items past
their next check, items with no next check and a turn that answered an
operator ask without an item, warning by default and blocking only as
a project opt-in. Five: status replies and `orient` built
from `follow list`, so what is missing from the ledger is visibly
missing from status. One implementation in the binary serves every
harness through adapters: Claude wires `.claude/settings.json`, Grok
`.grok/hooks/codeflow.json`; Codex has no session hook wired today, so
the work establishes what Codex supports and wires it or states the
gap. An orchestrator timer is an optional aid only.

This partly supersedes ADR-0076: planning still happens once, in
records, but the coordinator's running state no longer lives in the harness's
task tools; it lives in the clone and is read by any model.

Open, for the operator: the turn-end check
strength (advisory by default, or blocking in this repository); whether
a lane may open sub-lanes (proposed: only the coordinator, so one session sees
the whole tree; until decided, a lane that opens one records the parent
and warns); the Codex hook route (established by TSK-440). The design
page the operator reviewed is the planning session's "Orchestration
ledger and lanes" artifact (`JNdVjLApNR2UqttrtpiAZT`).

## Consequences

Easier: any session, model or seat reads one list; an interrupted lane
is recoverable from the clone, and its history shows which work it
carried; status replies are built from recorded
items, not memory; operator asks have an owner and a next check that
survive a session. Harder: every session must claim a role and keep a
heartbeat; the guards must keep allowing CodeFlow's writes under
`.git/codeflow/` while refusing direct edits; a project with several
clones has several ledgers, and the snapshot is the only way to share
one. Ruled out: a tracked ledger in history, a per-machine ledger, and
closure by hand-off. The guidance homes (the managed block,
cf-model-orchestrator, cf-herdr, cf-delegate, cf-method, the
per-harness notes, docs/architecture.md and the CLI help) change in
EPC-025.

## Architecture impact

`docs/architecture.md` gains one line: the clone's git directory holds
CodeFlow's coordination working state (`codeflow/follow/`, the ledger
and lane events) beside its ledger, locks and registry state; this is
written in EPC-025 (TSK-423), since this ADR is proposed in a planning
amendment and accepted by the operator.
