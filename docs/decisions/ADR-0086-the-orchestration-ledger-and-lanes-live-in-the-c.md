---
id: ADR-0086
uid: 6be2c341-340f-4add-9d89-ecd41ae5994b              # hidden record identity, written once by `new`; never edit
title: The orchestration ledger and lanes live in the clone
date: 2026-10-22
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md gains one line, the clone's git directory holds CodeFlow's orchestration working state (codeflow/follow/) beside its ledger, locks and registry state"
---

# ADR-0086: The orchestration ledger and lanes live in the clone

## Context

An orchestrating session owns a ledger of open commitments that
CodeFlow gives no shape, no closure rule and no home (issue 272). In an
adopting project a hand-written ledger stopped after nine days, items
the operator handed over went quiet, a merge of lane stocktakes gave
about 940 rows mostly duplicating tracked work, and a deleted lane
session lost the seats and reviews it was running. ADR-0076 plans work
in the harness's task tools, which are session-scoped and
vendor-specific, so they cannot hold state that must outlive a session
and be read by any orchestrating model. The operator shaped the answer
on 2026-10-22 and asked for the options and their costs on record; the
decisions below are the operator's, and three choices stay open.

## Decision

The orchestration working state lives in the clone, owned by CodeFlow
in schema, CLI, hooks and rules, and by the project in its entries.

Location, three options were weighed. A: clone-local, in the common
git directory (`.git/codeflow/follow/`), where CodeFlow's ledger JSONL,
locks and registry state already live; shared by every worktree of the
clone; untracked; never in history. B: a tracked file under
`project-management/`, which needs a pull request per change and is
too heavy at minute-level churn. C: a git-ignored folder in the root
checkout, readable as a plain file but bound to one checkout and lost
with it. Decided: A, with `codeflow follow list` as the table or
Markdown view and an opt-in tracked snapshot for a project that wants
history. One ledger per project (per clone), not per host, so any
orchestrating model or session reads the same list.

Format: JSON Lines events (add, update, close, and the lane events),
appended under a lock and validated against a schema, with a folded
current view derived from them; Markdown is only the human view
(`codeflow follow list --md`), because concurrent edits to one Markdown
file collide and cannot be validated. The layout under
`.git/codeflow/` is `follow/events.jsonl`, `lanes/events.jsonl`,
`roles/host.lease` and `ids/` for the `FU` and `LN` counters, beside
the existing `ledger/`.

Rows: `FU-NNN`, rising and never reused, with what, source, holder, a
CodeFlow id or the reason none exists, what it waits on, next check,
the running seat, worktree and head while work runs, and closure
proof; a row is open, waiting, due, closed (proof) or dropped (reason,
by the holder). A row waits on a task, a pull request, a lane, another
row, the operator, an outside event or a time, and CodeFlow derives
when it is due from those changing state; a merged pull request or a
completed task closes its linked rows with that proof; rows point at
tasks and never copy the task graph; a cycle among rows is refused. Any session may add a row. A holder is a role,
`host` or a lane id, never a session id; only the holder closes a row,
on proof (a merged pull request or commit, a recorded ruling, the
operator's answer); "handed to a lane" never closes one; a row linked
to a task closes with the task. A row that duplicates an open record is
refused until it is linked or the match is rejected.

Lanes: a session is a process; the host is the coordinating role; a
lane is a durable unit of delegated work; a seat is the harness
session attached to a lane, replaceable. A lane has an id
`LN-NNN-<slug>`, allocated per project, monotonic, never reused; it
records kind (build, review, research, consult), a scope, model, the
current seat (harness, pane, resume id, worktree, head), a heartbeat
and its rows. A lane often carries more than one task, sometimes a
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
labels carry lane ids and `cf-herdr` refuses a seat without one; a
build lane for a task opens only when `work next` reports the task
ready, and `work claim` opens or links the lane; a standalone session
is its own host. The host role is one session's lease with a
heartbeat. Two settings in the project configuration, placed by the
policy placement rule: `check interval`, how often the host surfaces
due rows (default 10 minutes), and `heartbeat timeout`, how long
without a sign of life before a host or seat counts as gone (default
30 minutes); the timeout must exceed the interval, because one long
command (a 25 to 45 minute full gate) refreshes no heartbeat while it
runs, and heartbeats are refreshed by every hook firing and every
`codeflow` command. Takeover is never automatic: the start hook reports
a stale host or seat, a session runs an explicit `adopt`, and a
returning session that finds its role or lane adopted stops and says
so. Rows of a
holder whose heartbeat has stopped show as orphaned at the next session
start and are adopted by the next session claiming that role. The host
sweeps all rows and owns operator asks; a lane sees and closes its own
and adds rows for what it hands back; every session sees operator-owned
rows.

Enforcement is five layers, and never a session timer; only the first
depends on the model remembering. One: the managed always-rule and the
orchestrator skill. Two: `codeflow hook session-orient` at session
start claims or reports the role and prints the rows owed, overdue,
orphaned and waiting on the operator, plus lanes with stale seats.
Three: the commands that do the bookkeeping (`work claim` opens or
links a lane, `cf-herdr` refuses a seat without a lane id, a merged
pull request or completed task closes linked rows with that proof,
`follow add` refuses duplicates). Four: a turn-end check that reports
rows past their next check, rows with no next check and a turn that
answered an operator ask without a row, advisory by default and
blocking under a policy key. Five: status replies and `orient` built
from `follow list`, so what is missing from the ledger is visibly
missing from status. One implementation in the binary serves every
harness through adapters: Claude wires `.claude/settings.json`, Grok
`.grok/hooks/codeflow.json`; Codex has no session hook wired today, so
the work establishes what Codex supports and wires it or states the
gap. An orchestrator timer is an optional aid only.

This partly supersedes ADR-0076: planning still happens once, in
records, but the host's running state no longer lives in the harness's
task tools; it lives in the clone and is read by any model.

Open, for the operator: the turn-end check
strength (advisory by default, or blocking in this repository); whether
a lane may open sub-lanes (proposed: only the host, so one session sees
the whole tree; until decided, a lane that opens one records the parent
and warns); the Codex hook route (established by TSK-440). The design
page the operator reviewed is the planning session's "Orchestration
ledger and lanes" artifact (`JNdVjLApNR2UqttrtpiAZT`).

## Consequences

Easier: any session, model or seat reads one list; an interrupted lane
is recoverable from the clone, and its history shows which work it
carried; status replies are built from recorded
rows, not memory; operator asks have an owner and a next check that
survive a session. Harder: every session must claim a role and keep a
heartbeat; the guards must keep allowing CodeFlow's writes under
`.git/codeflow/` while refusing direct edits; a project with several
clones has several ledgers, and the snapshot is the only way to share
one. Ruled out: a tracked ledger in history, a per-host ledger, and
closure by hand-off. The guidance homes (the managed block,
cf-model-orchestrator, cf-herdr, cf-delegate, cf-method, the
per-harness notes, docs/architecture.md and the CLI help) change in
EPC-025.

## Architecture impact

`docs/architecture.md` gains one line: the clone's git directory holds
CodeFlow's orchestration working state (`codeflow/follow/`, the ledger
and lane events) beside its ledger, locks and registry state; this is
written in EPC-025 (TSK-423), since this ADR is proposed in a planning
amendment and accepted by the operator.
