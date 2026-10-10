### Added

<!-- codeflow:release-impact minor -->
- **CI checks the release digests the project pinned.** The managed CI
  installers trusted the release's own `sha256.sum`, which anyone who can
  replace a release asset can replace too (sathyassn/codeflow#47).
  `codeflow update --pin <version>` now downloads a release's `sha256.sum`
  and its Linux and macOS archives, refuses any archive that does not match,
  and writes only `scaffold_version` and a `[scaffold_sha256]` table of
  digests to `.codeflow/project.toml`, for review in the pull request that
  raises the pin. Once the target pins that table, every installer (the
  gates, candidate and enforcing jobs and the shared GitLab, Bitbucket and
  generic script) requires the archive to match it and still checks
  `sha256.sum`; a table left from another version, a missing platform, a
  different digest or a table written in a form they do not read fails the
  job closed. CodeFlow now writes the values it serializes in a form they
  read, and `--pin` refuses, naming the line, when a line the project wrote
  would still be refused. A project with no table is checked
  against `sha256.sum` alone, with a warning, so a fresh `codeflow init` and
  the upgrade that adds the table still pass. `codeflow doctor --check
  ci-perimeter` names the check CI applies on the target and warns on a
  table CI would refuse. The first upgrade step is now
  `codeflow update --pin <version>`, then `codeflow update`.
