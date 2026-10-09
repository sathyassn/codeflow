### Added

<!-- codeflow:release-impact minor -->
- **`codeflow test --only` runs a gate in parts.** `--only <targets>` runs
  the named targets and their prerequisites, comma separated or repeated,
  so one gate can be split across parallel CI jobs. A limited run is
  recorded as not complete, so it never serves as a green base for
  `--since`, and a name that is not an enabled target of the mode is
  refused before any target starts. With `--all`, the named targets keep
  the full-strength checks of the epic close.
