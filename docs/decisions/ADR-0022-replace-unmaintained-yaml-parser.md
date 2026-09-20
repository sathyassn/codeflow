---
id: ADR-0022
title: replace the unmaintained YAML parser
date: 2026-07-14
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0022: replace the unmaintained YAML parser

## Context

Codeflow parses user-authored task, epic, policy, and configuration YAML at
multiple trust boundaries. Its direct `serde_yaml` dependency declares itself
unmaintained, so defects in that parser no longer have an upstream maintenance
path even though the public API remains convenient and well covered here.

## Decision

Use the maintained `serde_yaml_ng` continuation through the existing
`serde_yaml` crate alias. Keep the current call sites and serialized formats,
and require the full workspace tests, documentation build, and dependency
security audits to pass before accepting an upgrade.

## Consequences

- YAML behavior keeps the familiar Serde API while regaining an active upstream
  maintenance path.
- The package identity in `Cargo.lock` changes; downstream source imports do not.
- Future upgrades still require format-compatibility tests because handwritten
  YAML is a public input surface.

## Architecture impact

None. This replaces an implementation dependency behind the existing parsing
boundary; no module ownership, persisted schema, or command contract changes.
