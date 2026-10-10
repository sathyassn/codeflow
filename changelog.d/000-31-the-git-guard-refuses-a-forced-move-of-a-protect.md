### Fixed

<!-- codeflow:release-impact patch -->
- **The git guard refuses a forced move of a protected branch.**
  `git branch -f main HEAD~3` passed the guard, and the
  reference-transaction hook lets a rewind behind the remote through. The
  guard now refuses `git branch -f`, `-M` and `-C`, `git checkout -B`,
  `git switch -C` and `git worktree add -B` aimed at a protected branch
  under `git.local_ref_protection`, as it already refused `git
  update-ref`. It reads flag clusters such as `-fv` and abbreviations
  such as `--force-c`, resolves `@{-1}` and `@{upstream}` in the target
  repository, and refuses a forced move it cannot resolve, or one whose
  expression an earlier git command on the same line may change.
