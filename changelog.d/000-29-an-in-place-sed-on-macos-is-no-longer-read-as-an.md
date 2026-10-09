### Fixed

<!-- codeflow:release-impact patch -->
- **An in-place `sed` on macOS is no longer read as an edit of the
  enforcement files.** In a worktree under `.claude/worktrees/`, the git
  guard refused `sed -i '' ...` on any file, and an empty operand of `rm`
  and the other write commands, as an edit of the repository's enforcement
  files. It now reads GNU and BSD sed's own option grammars (BSD `-i` and
  `-I` take a separate backup suffix, `-l` is a flag), never treats an
  empty argument as a path, and judges a worktree nested in the main
  checkout's `.claude/` by its own files. Writes to the worktree's own
  `.claude/settings.json` or `.codeflow/policy.json` are still refused,
  and so is any `sed` whose script, options or `-f` script file names an
  enforcement path; a plain read such as `sed -n p <file>` passes.
