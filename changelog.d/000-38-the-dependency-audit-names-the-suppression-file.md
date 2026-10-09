### Fixed

<!-- codeflow:release-impact patch -->
- **The dependency audit names the suppression file osv-scanner reads.**
  When the CI template's audit step fails, it told you to record a
  justified suppression in `.osv-scanner.toml`, a name osv-scanner never
  reads. It now names `osv-scanner.toml` in the same directory as the
  lockfile it covers. A new docs portal starts with that file, holding a
  header and no ignores, so a justified suppression has a home.
