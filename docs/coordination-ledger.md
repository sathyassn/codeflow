# Coordination ledger schema

The coordination ledger is the clone-local working state of the sessions
that coordinate work: follow-ups (`FU-NNN`), lanes (`LN-NNN`), work streams
(`WS-NNN`) and coordinator claims. ADR-0086 decides it; this page is the
field-level reference of what it stores. The ledger is an aid, never a
control: nothing in records, pull request bodies, CI or git hooks reads it,
and it never writes a record. `codeflow follow` is its command
([CLI reference](cli.md)).

## Where it lives

The store is in the clone's common git directory, found through Git's own
discovery, so every worktree of a clone shares one ledger and nothing is
ever committed:

```text
<git common dir>/codeflow/coord/
  events.jsonl    the one log: every event below, one sequence, append-only
  status/         liveness: one overwritten file per actor instance
  counters.json   reserved FU and LN numbers; gaps allowed, never lowered
  coord.lock      the stable sidecar lock; never removed or replaced
  instance.json   this ledger's UUID; FU, LN and WS ids are local to it
  quarantine/     torn tails and damaged suffixes moved aside, kept
```

The directory is created `0700` and its files `0600`; a symbolic link in
place of the directory or a file is refused. The supported live scope is
one clone on one machine; shared network filesystems are not supported.
An id exported outside the clone is qualified by the instance UUID
(`<uuid>/FU-007`).

## Entities and their references

Every entity refers to another by id only. Every event is one line in the
one log, with its subject id and its place in the one sequence.

```text
                     +------------------------------+
                     | Work stream  WS-NNN          |
                     |   slug, scope (links only),  |
                     |   closed                     |
                     +------------------------------+
                        ^            ^           ^
          stream (id)   |            |           | stream (id), or "intake"
   +--------------------+--+   +-----+--------+  |
   | Coordinator claim     |   | Lane  LN-NNN |  |
   |   stream (id)         |   |   stream (id)|  |
   |   actor instance id   |   |   scope      |  |
   |   generation          |   |   generation |  |
   +-----------------------+   +--------------+  |
                                ^          ^     |
                     lane (id)  |          | holder: "coordinator" of the
              +-----------------+--+       | stream, or a lane id
              | Seat               |       |     |
              |   seat instance id |  +----+-----+--------------------------+
              |   kind, harness    |  | Follow-up  FU-NNN                   |
              +--------------------+  |   stream (id), holder (coordinator  |
              | Attempt            |  |   or lane id), waits_on (FU, LN ids)|
              |   lane (id), task, |  |   links: context | prerequisite |   |
              |   worktree, branch,|  |   fulfils -> task, PR, commit,      |
              |   revision         |  |   branch, record, FU, LN, time      |
              +--------------------+  +-------------------------------------+

   events.jsonl:  sequence 1, 2, 3, ...   each line names its subject_id
                  (FU-NNN, LN-NNN or WS-NNN) and is judged by the same
                  rules when written and when replayed
```

## The envelope

Every line of `events.jsonl` is one JSON object with these fields. Unknown
fields are refused.

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | integer | `1`. A newer version makes the ledger read-only for an older build. |
| `event` | string | One of the event kinds below. |
| `subject_id` | string | `FU-NNN` for follow-up events, `LN-NNN` for lane events, `WS-NNN` for stream and coordinator events. |
| `sequence` | integer | Assigned under the lock; exactly one above the previous line. Order comes from this alone. |
| `purged` | boolean, optional | `true` on an event `follow purge` rewrote: its key and source reference are digests. |
| `recorded_at` | string | UTC, `YYYY-MM-DDTHH:MM:SSZ`, for people; never for order. |
| `recorded_by` | object | Who recorded it; see below. |
| `comment` | string, optional | Free text, at most 1,000 characters. |
| `idempotency_key` | string, optional | A retried operation with the same key is recorded once. |
| `expected_revision` | integer, optional | The subject's revision the writer read; a stale one is refused. |
| `data` | object | The event's fields, by kind. |

`recorded_by` has `role` (`coordinator`, a lane id or `operator`),
`actor_instance` (always), `generation` (a coordinator's or lane's claim
generation), `stream` (a coordinator's stream), and `harness`, `model` and
`session`, each written as `unknown` when it is not known. A shell that
carries a harness's agent marker cannot record as the operator; it records
an operator's answer under its own role with `answer_source`. That check is
not authentication: see "The operator" under [The command](#the-command).

## Records

### Follow-up

Folded from `item_added` and every later item event. A full record for a
cold reader.

| Field | Required | Meaning |
|---|---|---|
| `title` | yes | One line, at most 200 characters. |
| `description` | yes | What this is about, at most 4,000 characters. |
| `done_when` | yes | The observable condition that finishes it. |
| `source` | yes | `operator`, `coordinator`, `lane`, `review`, `issue` or `outside`. Never updated. |
| `source_reference` | where one exists | The source's own reference. With `source`, the identity an exact duplicate is refused on. |
| `stream` | yes | A stream id, or `intake` until a coordinator takes it. |
| `holder` | yes | `coordinator` (the stream's coordinator, no lane) or a lane id. |
| `links` | one of these two | Typed links: `context`, `prerequisite` or `fulfils`, each to a target below. |
| `no_link_reason` | one of these two | Why it links to nothing. |
| `schedule` | no | The schedule the adder asked for (`30m`, a UTC time); part of the operation's identity. |
| `next_check` | yes | The effective next check, UTC; the default is 10 minutes after the add. An item that waits on a prerequisite is rechecked at least daily. |
| `waits_on` | no | Follow-up or lane ids, possibly another stream's; read-only. |
| `awaiting_operator` | no | A condition the operator must settle; never a holder. |
| `rejected_suggestions` | no | `reference` and `reason` of each suggested similar item the adder rejected. |
| `observed` | no | The links' states when last recorded (`link`, `state`, `version`). |

A link target is one of: `record` (`id`, `uid`, `repository`), resolved
through the record files by id and UID, never by title; `pull_request`
(`number`, `repository`); `commit` (full `sha`, `repository`); `branch`
(`name`, `remote`, `repository`); `follow_up` (`id`); `lane` (`id`); `time`
(`at`). A `fulfils` link names a task, a pull request, a commit or a branch.

Folded state adds: the item state (`open`, `closed`, `dropped`), its
revision (events applied), every closure proof (kept after a reopen) and
the last check.

### Work stream

`WS-NNN`, local to the ledger: `slug`, `scope` (links only, at most 32),
`closed`. One live coordinator per stream.

### Coordinator claim

The lease on a stream: `stream`, the holder's `actor_instance` and its
`generation`. A claim takes the next generation; an adoption compares the
expected holder and generation and raises the generation by one; a stale
writer is refused.

### Lane

`LN-NNN`, rising and never reused: `stream` (exactly one), `slug`, `scope`,
`generation` (1 at open, raised by each adoption), the attached seat, the
last report and the outcome when closed (`done`, `abandoned`, `superseded`
with a successor lane).

### Seat

The harness session attached to a lane: `seat_instance`, `kind`
(`external`, `resumable` or `in_process`) and `harness`. A lane holds one
seat at a time; its instance and the lane's generation bind the lane's
actor.

### Attempt

One run of work in a lane: the lane id, task, worktree, branch, revision,
expected result and native provenance. Attempts arrive with the lane
commands; this schema version records none yet, and validates no attempt
event.

## Events

Each section lists the event's `data` fields. Every free-text field is
size-bounded and scanned for secrets after decoding; control and format
characters are refused. Who may record each follow-up event is the
permission matrix: any bound actor adds; the holder (or the operator)
updates, checks, hands over, closes and purges; the holder, the stream's
coordinator or the operator drops (always with a reason) and reopens; any
coordinator takes an intake item.

### `item_added`

The full follow-up record, as in the table above. Subject: a newly reserved
`FU-NNN`.

### `item_updated`

Any of `title`, `description`, `done_when`, `links` (replaces),
`no_link_reason` (replaces the links), `next_check`, `waits_on` (replaces),
`awaiting_operator` (empty clears), `observed`. Never `source`. Changes
nothing is refused.

### `item_checked`

`observed` (what was seen, in words), `next_check`, `observations` (each
link's `link`, `state` and `version`), optional `answer_source` when the
holder records someone else's answer.

### `item_handed_over`

`to_holder` (`coordinator` or a lane id of the destination stream), optional
`to_stream` (a stream id or `intake`).

### `item_closed`

The evidence: `kind` (`asserted` or `machine_verified`), `reference` a fresh
session can open, `source` of the observation, `repository`, `target`,
`revision` (both required when machine-verified) and `observed_at`.

### `item_dropped`

`reason` and `disposition` (what happens to what the item leaves).

### `item_reopened`

`reason`. The old proof stays in history.

### `item_purged`

`events`, the number of the item's events whose free text was replaced.
Who purged it and when are the envelope's; no content is recorded. The
holder or the operator.

### `stream_opened`

`slug` (optional) and `scope`. Recorded by the operator or by the stream's
own coordinator.

### `stream_scope_changed`

`slug` (optional) and `scope`. The stream's coordinator or the operator.

### `stream_closed`

`reason`. Ends the coordinator claim.

### `lane_opened`

`stream`, `slug` (optional), `scope`. Only the stream's coordinator or the
operator. Subject: a newly reserved `LN-NNN`.

### `lane_scope_changed`

`slug` (optional) and `scope`; never amends tracked records.

### `seat_attached`

`seat_instance`, `kind`, `harness`. The lane must have no seat.

### `seat_detached`

`seat_instance` (the attached one) and `reason`.

### `lane_reported`

`summary`. Recorded by the lane's own seat or the stream's coordinator.

### `lane_adopted`

`expected_holder` (the current seat instance, or `none`),
`expected_generation`, `new_holder`, `generation` (one above).

### `lane_closed`

`outcome` (`done`, `abandoned` or `superseded`), `detail` (the evidence or
the reason), `disposition` of the lane's open follow-ups, and `successor`
exactly when superseded.

### `lane_handed_over`

`from_stream`, `to_stream` and `accepted_by` (the receiving coordinator's
instance). The lane's open follow-ups move with it.

### `coordinator_claimed`

`generation`, one above the stream's last. Recorded by the claimer.

### `coordinator_adopted`

`expected_holder`, `expected_generation`, `new_holder`, `generation` (one
above). Recorded by the adopter or the operator.

### `coordinator_released`

No fields. Recorded by the holder or the operator.

## The command

- **Who acts.** A writing verb acts as the role named by
  `CODEFLOW_COORD_ROLE` (`coordinator`, a lane id or `operator`) with
  `CODEFLOW_COORD_INSTANCE` and `CODEFLOW_COORD_GENERATION`, and
  `CODEFLOW_COORD_STREAM` for a coordinator; the ledger checks the claim
  against its own log and refuses a stale one. With no role set, a caller
  only reads.
- **Binding is not authentication.** The `CODEFLOW_COORD_*` binding is
  cooperative ownership among sessions that state their own role truthfully.
  The instance id and generation are readable in the log, so any process
  that can run `codeflow` can claim to be any actor. The checks catch stale
  and mistaken writers, not a hostile one; nothing in the ledger is a
  security boundary.
- **The operator.** The operator role is accepted only when standard input
  and standard error are both a terminal and no harness marker
  (`CLAUDECODE`, `CODEX_SESSION_ID`, `CODEX_THREAD_ID`, `CODEX_CI`,
  `GROK_SESSION_ID`) is set. This stops an ordinary agent shell, which has
  no terminal and carries its marker. It does not stop a process that
  unsets the marker and runs `codeflow` under a pseudo-terminal (`script`,
  a pty library): nothing tells a person from an agent working in the same
  account. Treat an operator-recorded event as stated by whoever ran the
  command, not as proof that a person did. An agent records an operator's
  answer under its own role with `--answer-source operator`.
- **Closing.** `follow check` closes an item only when every `fulfils`
  link is done on its target and every prerequisite and `waits_on` id it
  declares is met: the task `complete`, with an acceptance block that passes
  the acceptance rules and is bound to its reviewed commit as CI binds a
  completion as its landing gate bound it, on the tip of the landing line its
  record declares, with the UID the link pinned. The completion that counts
  is the newest transition on that line's first-parent history: a commit
  where the record (found only at the record layouts, by its UID) goes from
  not complete to complete, or changes its acceptance block while complete.
  A move that changes neither is no transition. A direct transition is
  judged at its own commit; a merge at the head it brought in, which must
  hold the same completion (a merge that edits the record is judged at the
  merge and cannot bind). The review must be on the judged head's
  first-parent line, and for a merge no reopen of the task may lie on the
  target's first-parent history between the review and the merge. A landing line is the one
  remote-tracking ref the declared target names, by the same remote rules
  `work start` uses (`origin/main` is origin's, never another remote's; a
  bare name is `origin`'s, never chosen by a local branch's upstream
  setting; an absent tracking ref is unknown), for `main`, `master` or an `integration/`
  line that a task record on the default target names as its target. A
  local branch never counts, nor a pushed line no record on the default
  target names. A pull request counts when the host reports it merged into
  such a line; a commit when it is in the default work target; a branch when
  a remote this clone configures shows it deleted. A link to another
  repository, a completion on a work branch and an observation that cannot
  be made never count.
- **Clones without a remote.** Task-completion proof needs a configured
  remote's tracking ref, so in a clone with no remote a task link never
  closes on its own. Commit proof is the stated exception, in line with the
  ledger's clone-local scope: it is judged against the default work target,
  which in such a clone is its local default branch. Pull request proof
  comes from the host. Asserted closure by the holder is always available. The `done_when` text is for people; the
  machine condition is the links. `follow close` records asserted evidence.
- **Reopening.** A later `follow check` reopens a machine-verified closure
  whose condition is now definitely unmet: the task reverted, replaced or no
  longer accepted, the pull request reopened, the commit gone from the
  target, the branch recreated, or a prerequisite or `waits_on` follow-up
  or lane open again. The old proof stays in history.
- **Observations.** `gh` and `git ls-remote` each run with a 20 second
  limit that covers the process, its output and any descendant holding its
  output open (killed with its process group); output is capped at 1 MiB.
  One that does not answer counts as unknown. A branch link contacts only a
  remote this clone configures, over https or ssh, with the stock upload
  pack and the stock ssh client.
- **Idempotency.** A key is bound to the operation's kind, subject, actor
  and content. Only fields a retry recomputes are left out: an add's
  absolute next check is, but the schedule it was asked for (`--next-check
  30m`) is content. The same key and content again records nothing;
  the same key with other content or another actor is refused (exit 7). A
  key whose item was purged is acknowledged without writing anything. An operation of
  several events gives each its own key, so a retry after an interruption
  writes exactly the missing events.
- **Reads.** `follow list` and `follow snapshot` never write, take a lock or
  refresh a heartbeat. A damaged ledger only warns and shows its valid
  prefix.
- **Exit status.** 0 done, 1 refused, 2 no repository or unusable input,
  3 busy (another process held the lock past the bounded wait), 4
  read-only (a damaged log), 5 unwritten (a denied write), 6 unavailable (a
  denied read), 7 key conflict.
- **Diagnostics.** A refusal names the field, never a secret-shaped value:
  such a value is withheld or shown as `[redacted]`.
- **Not a control.** No gate, hook, CI check or landing reads the ledger.
  Records and pull request bodies never cite FU, LN or WS ids:
  `codeflow validate --docs` flags a task, epic or ADR record that does, and
  `codeflow ci` blocks such a pull request body.

## Transactions and damage

- One append is one write of whole lines under `coord.lock`, synced before
  success is reported. Numbers are reserved in `counters.json` before the
  events that use them are written.
- A writer waits a bounded time for the lock (10 seconds for a command,
  2 seconds for a hook) and reports busy, which is distinct from empty.
- Readers take no lock, create nothing and fold the committed prefix: every
  complete line.
- A torn tail (bytes after the last line break) is moved to `quarantine/`
  under the lock before the next append.
- Interior damage, a duplicate or decreasing sequence, a broken transition
  or an unknown newer schema version makes the ledger read-only; reads
  still show the valid prefix with a warning. `codeflow follow repair`
  (the operator) moves the damaged suffix to `quarantine/`.
- `codeflow follow purge FU-NNN` (the holder or the operator, judged under
  the lock) replaces every free-text string of the item's events with
  `[purged]`, and its source reference and idempotency keys with digests
  that keep their identity without their text, so a stale retry is still
  recognised and the same source is still a duplicate. Each rewritten event
  carries `"purged": true`; that flag, never a text prefix, marks its
  identity as purge-made, so user text that looks like a digest is still
  purged and a repeated purge changes nothing more. It keeps ids, link
  identities, times and order, removes only the item's own lines from
  quarantined fragments (judged by each line's `subject_id`, in any
  spacing, even beside a damaged byte; other items' lines stay byte for
  byte), names any fragment it cannot judge instead of claiming it purged,
  and records an `item_purged` event with who and when, never the text. The
  operator may also purge an item that survives only in quarantine; that
  item is not in the log, so only its quarantined lines change and no event
  is recorded. A quarantined line that holds another item's `subject_id`
  beside this one (two events joined by a lost line break) is kept and
  named, not split.
- On a write, an existing ledger directory or file must belong to the user;
  group and other access is removed. A failed directory sync is reported,
  never ignored.
- There is no compaction. The read path folds 10,000 events once per
  invocation; the measured figure is recorded with the release evidence.
