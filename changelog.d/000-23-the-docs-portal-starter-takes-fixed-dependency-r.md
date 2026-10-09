### Fixed

<!-- codeflow:release-impact patch -->
- **The docs portal starter takes fixed dependency releases.** New
  advisories against `postcss-selector-parser`, `smol-toml`,
  `source-map-js` and `http-cache-semantics` failed a blocking dependency
  audit in a project with the portal. The starter's `package.json` now overrides `postcss-nested` to
  8.0.1, and its lockfile moves to `postcss-selector-parser` 7.1.6,
  `smol-toml` 1.9.0, `source-map-js` 1.2.2 and `http-cache-semantics`
  4.3.0. `codeflow update` installs both files; the portal's recorded
  runtime scripts are unchanged.
