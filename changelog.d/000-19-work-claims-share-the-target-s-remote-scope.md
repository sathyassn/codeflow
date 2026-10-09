### Fixed

<!-- codeflow:release-impact minor -->
- **Work claims share the target's remote scope.** Archive and other unrelated
  remote branches no longer prevent a claim or mark a task active. They remain
  visible as information. Local branches, origin and the target's fetch remote
  still count; projects can opt in other remotes with `git.claim_remotes`.
  The shipped policy file does not list the key, so neither `init` nor
  `update` writes it and an older binary never meets it; a project that sets
  it runs 3.1.0 or later locally and in CI.
