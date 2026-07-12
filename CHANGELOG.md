# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Enforcement is the floor; the tiers scale project-management (ADR-0019).**
  The `--minimal` tier now installs the complete four-plane enforcement floor,
  not just the pre-commit secret scan: the `commit-msg`, `pre-push`,
  `pre-merge-commit`, and `reference-transaction` git hooks, the scaffolded CI
  workflow (`codeflow-ci.yml`), the in-session `git-guard`/`exec-guard` +
  orient/summary hooks (`.claude/settings.json` and the `.codex/` starter), and a
  new lean `CLAUDE.md` all moved into the minimal tier alongside the armed policy
  it already shipped. `--standard` and `--full` are unchanged; they still add the
  method (cf-* skills, reviewer agents, the pipeline), the traceability spine, and
  project-management on top. The change is additive — nothing is removed from any
  tier.
- **Existing `--minimal` installs gain the enforcement floor automatically on
  their next `codeflow update`.** The update reconciliation installs manifest
  entries that are in-tier but missing on disk, so an old-minimal repo's next
  update adds the moved hooks, CI, settings, and Codex starter and records them —
  no re-init required.

### Added

- **Two working principles in both the minimal and full agent contracts.**
  *"Think independently — not a yes-man"*: a request, opinion, claim, or proposed
  approach — the operator's included — is owed analysis and evidence, not
  agreement; the operator still makes the final call, but agreement without
  examination is a failure mode, not deference. *"Think in depth, not at the
  surface"*: chase the implication chain ("and therefore? …") to the fundamental
  that decides the matter, and trace how each order ripples across the related
  domains, not just the immediate area.

## [2.1.0] - 2026-07-05

### Added

- Cross-harness skills: the `cf-*` skills install to `.agents/skills/` (read by
  Codex) alongside `.claude/skills/`, so one skill set serves multiple coding
  CLIs. Claude Code merged custom commands into skills (v2.1.101); codeflow now
  ships skills only.
- The commit-msg gate flags a mis-cased `BREAKING CHANGE:` / `BREAKING-CHANGE:`
  footer, so a breaking change is never silently downgraded to a minor bump.

### Fixed

- `codeflow update` no longer silently discards merged-in user edits to a
  managed file on the *next* update. After a clean 3-way merge the manifest now
  records the pristine shipped hash (restoring the invariant `recorded ==
  hash(baseline)`), so a merged file stays classified user-modified and is
  re-merged rather than overwritten with the shipped version.
- `codeflow update` reconciles orphaned managed files: an artifact removed or
  renamed upstream (e.g. a command that became a skill), and its baseline and
  manifest record, is pruned when unmodified instead of lingering and colliding
  with its renamed replacement. User-modified and user-owned files are never
  deleted (ADR-0011).

### Changed

- Shipped scaffold content and documentation corrected for the commands→skills
  rename and for install/enforcement accuracy ahead of the public release.

## [2.0.0] - 2026-07-03

2.0.0 is a complete Rust rewrite of codeflow. The 1.x line (a shell/Node tooling
set) shares no code with it and is preserved at the `v1-final` tag.

### Added

- Single `codeflow` binary (crates `codeflow-core` + `codeflow-cli`) with an
  embedded scaffold, installed into any repo via `codeflow init`.
- Policy-driven git enforcement across four planes — git client hooks, an
  in-session PreToolUse guard, CI, and remote branch protection — all reading a
  single `.codeflow/policy.json`.
- Scaffolding with three ownership classes (managed, managed-region,
  user-owned), 3-way-merge `codeflow update`, and version-skew detection.
- Knowledge model: capabilities registry, ADRs, epics/specs, an automatic
  ledger, and `codeflow recall` full-text search.
- Composable pipeline workflow (build → independent review → verify) and
  cross-vendor delegation (`cf-delegate` / `cf-consult`).
- CLI surface: `init`, `update`, `test`, `validate [--docs]`, `status`,
  `recall`, `orient`, `doctor`, `integrate`, `remote`.
- cargo-dist release pipeline with prebuilt binaries for macOS (arm64/x64) and
  Linux (x64) and a shell installer.

[Unreleased]: https://github.com/sathyassn/codeflow/compare/v2.1.0...HEAD
[2.0.0]: https://github.com/sathyassn/codeflow/releases/tag/v2.0.0
