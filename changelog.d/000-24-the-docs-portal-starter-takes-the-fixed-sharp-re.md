### Fixed

<!-- codeflow:release-impact patch -->
- **The docs portal starter takes the fixed `sharp` release.** Advisory
  GHSA-wq5f-xc86-pv6w (high) affects `sharp` 0.35.4, the version the
  starter pinned, and failed a blocking dependency audit in a project with
  the portal. The starter's `package.json` now pins `sharp` to 0.35.5, and
  its lockfile moves `sharp` and its `@img/*` packages to the matching
  releases, with `@img/sharp-libvips-*` at 1.3.4. `codeflow update`
  installs both files; the portal's recorded runtime scripts are
  unchanged.
