### Fixed

<!-- codeflow:release-impact patch -->
- **The release binary reports a clean build.** The 3.0.0 binaries print
  `dirty=true` in `codeflow --version` although they were built from the
  tagged source: the release job writes cargo-dist's manifest into the
  checkout before it builds, and the build counted that untracked file.
  The file is now ignored, so a release build reports `dirty=false`.
