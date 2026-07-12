# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/sathyassn/codeflow/compare/v2.0.0...HEAD
[2.0.0]: https://github.com/sathyassn/codeflow/releases/tag/v2.0.0
