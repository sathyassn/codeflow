### Fixed

<!-- codeflow:release-impact patch -->
- **CodeFlow's own fixture clones no longer race with automatic Git
  maintenance.** Hosted CI failed `readiness_journey` and
  `read_commands_offline` at random with "hardlink different from source"
  (2026-10-05 and 2026-10-06): a test fixture cloned a repository by copying
  its object files while the detached `git maintenance run --auto`, started
  by a commit in `codeflow init`, rewrote them. Every Rust fixture clone now
  goes through Git's transport (`git clone --no-local`) in one private helper
  crate, `codeflow-fixture`, and `.cargo/config.toml` turns automatic
  maintenance and garbage collection off for every process a test starts. A
  contract test refuses a new local clone, a `git-*` program spawned outside
  the git constructor and an unlisted environment override. The binary, the
  scaffold and the docs do not change; the fix concerns contributors and
  CodeFlow's own CI.
