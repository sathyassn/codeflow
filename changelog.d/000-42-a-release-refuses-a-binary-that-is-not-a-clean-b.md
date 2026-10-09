### Fixed

<!-- codeflow:release-impact patch -->
- **A release refuses a binary that is not a clean build.** Before a
  release is hosted, and on every dry run, each platform archive is opened
  and its `codeflow` binary must identify as the release version at the
  release commit with `dirty=false`. A dirty build, a build from another
  commit, or an archive without a binary stops the release.
