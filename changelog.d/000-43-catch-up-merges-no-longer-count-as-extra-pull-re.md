### Fixed

<!-- codeflow:release-impact patch -->
- **Catch-up merges no longer count as extra pull requests.**
  `codeflow validate --docs` warned that most completed standalone tasks
  were "completed by N pull requests", because it counted every merge whose
  subject names the task branch, including the merges of the target into
  that branch that the rules ask for. It now counts only merges that brought
  the task branch in. With the default merge subjects (GitHub's "Merge pull
  request", git's and GitLab's "Merge branch"), a task that landed once
  draws no warning and a task that really landed twice still does. A merge
  with a custom subject may still be miscounted, and a task landed by
  squash, rebase or `codeflow integrate`, which write no merge, is not
  counted, as before.
