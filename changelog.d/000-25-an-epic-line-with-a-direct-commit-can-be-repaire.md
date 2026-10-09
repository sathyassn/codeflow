### Fixed

<!-- codeflow:release-impact patch -->
- **An epic line with a direct commit can be repaired and land.** A commit
  made directly on an `integration/EPC-*` line used to block the line's pull
  request to `main` for good, and the only ways out were a force push or a
  new line name. The epic record now takes a `line_adoptions` list: each
  entry names the commit by its full id, why it belongs and where it was
  reviewed. CI accepts the line once that entry has landed by merge, or is
  already on the target, and prints each adopted commit on the class line
  for the reviewer. The pre-push hook now refuses a push that adds an
  unadopted direct commit to an epic line, with a `git reset --keep` remedy
  that moves the work into a pull request. Older direct commits outside the
  pushed range do not block further merges (issue 85).
