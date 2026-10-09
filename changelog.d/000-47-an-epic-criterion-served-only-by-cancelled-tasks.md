### Fixed

<!-- codeflow:release-impact patch -->
- **An epic criterion served only by cancelled tasks can close.** `codeflow
  epic status <EPC> complete` refused such a criterion, while a criterion
  no task serves could close on the epic's own acceptance block, so the
  only way out was a new task that cited evidence already in hand. The
  epic's own block now proves it under the same rules: verified with its
  evidence, or waived with its planning commit, and the journey verified
  for a journey criterion. A cancelled task still never verifies a
  criterion, and a ticked checkbox does not either. The epic's own block,
  for criteria no task serves as well, is now bound as a task's block is,
  in `epic status`, `codeflow ci` and the release-line check: before, any
  commit-shaped value passed as its reviewed commit or a waiver. Its
  reviewed commit must exist, with only the epic's status and Closeout
  changed after it, and a waiver must name a planning-only commit that
  amends that criterion and that the reviewed commit contains. An epic
  close that names a fabricated or stale review, or a waiver that is no
  such amendment, is now refused, and the refusal prints the epic's
  repair: correct the block and have it reviewed, then rerun `codeflow
  epic status <EPC> complete --acceptance <file>` for an open epic, or,
  for an epic the pull request already completes, replace the block in
  its Closeout by hand in that pull request; an epic is never reopened.
  The cf-method project-organization reference states the same route.
