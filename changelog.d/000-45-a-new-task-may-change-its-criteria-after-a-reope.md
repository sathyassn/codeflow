### Fixed

<!-- codeflow:release-impact patch -->
- **A new task may change its criteria after a reopen in its own pull
  request.** A standalone task whose record exists only on its branch,
  completed, reopened and given another criterion there, could not be
  completed again: `codeflow task status` and `codeflow ci` refused with
  "a reopened task keeps its criteria as the anchored target has them",
  though the target holds no criteria to keep. Such a task now completes
  with its new criteria. A task that landed on the target still keeps
  its criteria across a reopen, also when the branch moves its record to
  another layout, renumbers it with its uid kept, or retargets it away
  from `main` or from the integration line it was planned on, and
  whether the target is read from a stale local branch, an upstream on
  another remote or an older comparison base. A task is new only when no
  other branch adds or edits its record, so rewriting the branch's own
  history cannot hide a recorded task; the refusal names the branch that
  records it. The default branch is the
  one `origin/HEAD` names, else `main` or `master`. A clone that lacks a
  target the task's record names or the default branch cannot tell, so
  it refuses the change and names the branch to fetch, or explains how
  to record `origin/HEAD` when it finds no default branch.
