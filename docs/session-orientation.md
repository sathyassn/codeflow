# Integration lines in the session digest

`codeflow orient` and the `session-orient` SessionStart hook are read
commands and never fetch. They list the `integration/*` refs of `origin`
(or the sole configured remote) as last fetched, labelled `remote`; run
`git fetch` to refresh them. With no remote, or no fetched integration
refs, they list local lines labelled `local`.
The protected target is the remote's default branch, falling back to
`main` or `master`; local rows use local `main` or `master`.

## Reading a line

Each row shows commits behind the target, the latest own sync merge date
or creation state, and the oldest nonterminal task's age in days. Task
records are read from the line's committed tree. A task record with a
missing or invalid `created` date is left out of the age, and a
`line records:` row after the line names it. A sync is an own
first-parent merge with another parent on the target; its date is the
merge commit's date. Inherited target merges do not count.

Creation is known only from a local ref-creation reflog entry. Copied or
renamed reflogs cannot establish creation of the current ref. Remote
creation is `unknown`: a tracking reflog records a fetch, not creation.
The `oldest divergent commit` date is labelled separately and never
stands for creation. A line without own commits says `fresh, no own
commits`, regardless of the target commit's age.

## Sync reminders

`git.line_sync_interval` sets the reminder interval in days; `0` (the
default) disables it. A line past the interval since its last sync, or
its evidenced creation if never synced, gets a reminder naming
`codeflow line sync <line>` (pending TSK-421). Fresh lines and never-synced
lines with unknown creation get no time-based reminder. Orient never
merges, pushes or closes a line.

## Open epics

The digest also lists each open epic and its started-task count. With
durable tracking, this counts distinct nonterminal tasks with visible,
unlanded branch claims, including a claim before its first commit. In
legacy records without durable tracking, it counts `in_progress` tasks.
It does not read coordination streams. These inventories are complete;
the base digest's 30-line guideline does not truncate their rows.

See the [CLI reference](cli.md) for the other commands and exit contracts.
