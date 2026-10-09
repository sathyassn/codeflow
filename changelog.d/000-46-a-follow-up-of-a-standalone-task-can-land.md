### Fixed

<!-- codeflow:release-impact patch -->
- **A follow-up of a standalone task can land.** `codeflow task new
  --follow-up-of` ran only on a `plan/` branch, but a planning pull request
  must name an epic, and a standalone task has none, so no branch or
  `Task:` line could land the record. A standalone task's follow-up is now
  a standalone task too: cut a task branch from the target, file the
  follow-up there, fill in and commit its record, then run `codeflow work
  claim`, which renames that branch to `task/TSK-NNN-<slug>` and pushes it.
  Its record lands with its work in one pull request. A standalone task
  never uses a `plan/` branch, and the command now refuses on one and names
  that route. A follow-up of an epic task still rides in the epic's batched
  amendment on a `plan/` branch.
