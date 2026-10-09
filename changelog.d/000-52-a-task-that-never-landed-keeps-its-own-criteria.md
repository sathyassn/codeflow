### Fixed

<!-- codeflow:release-impact patch -->
- **A task that never landed keeps its own criteria change across a
  reopen.** A task the target records, but has not completed, may change
  its criteria in its own pull request, and CI prints the change as a
  note. When such a task completed on its branch and was then reopened to
  take a merge of its line, its second completion was refused with "a
  reopened task keeps its criteria as the anchored target has them", the
  same change accepted one completion earlier
  ([#67](https://github.com/sathyassn/codeflow/issues/67)). The freeze now
  follows landing evidence: a reopened task keeps its criteria when any
  version of its record in the judged target's history is complete, keeps
  an acceptance block or records a reopen, however the target changed or
  deleted the record later, and it keeps the criteria of the newest
  judged version; a deleted record added back reopened is a reopen. Otherwise `codeflow task status` and `codeflow ci`
  accept the change and CI prints it for the reviewer. A history that
  cannot prove the task never landed (a shallow clone, grafts or replace
  refs, a record or tree that does not read, a task file that does not
  parse and may name the task, one id with two uids, or lines of history
  that disagree) refuses a criteria change and nothing else. A landed task's criteria still change
  only through a planning amendment that names its epic, which now flags
  any landed task and reads a record the target deleted from its history;
  a local branch or remote-tracking ref can still only make the check
  stricter, and readiness still reads a predecessor's current status. The
  entry on new tasks above now holds for every task whose completion never
  landed.
