---
id: ADR-0031
uid: 76c88d4f-54e4-48ef-919e-af92cf352f24
title: safe adaptive test setup and fail-closed configured gates
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: The existing test setup adapter now consumes release-embedded templates, limits automatic detection to root markers, preserves populated or malformed project configs, and treats configured-gate parse failures as violations; legacy structural blocks remain loadable but explicitly unenforced.
---

# ADR-0031 — safe adaptive test setup

## Context

The core already had template, target-append, and detection helpers, but the CLI
exposed only automatic detection and source-tree tests supplied template paths.
Automatic setup treated a malformed config as replaceable, pre-push downgraded
the same parse failure to a skip, and detection emitted structural rules even
though the public gate did not execute the parked validator. The schema also
named CI variables the runner never read. These gaps made installed behavior
less safe and less honest than the internal APIs suggested.

## Decision

Keep one CLI surface: `codeflow test setup`. Its optionless path detects only
root stack markers and may fill an absent or empty config. It never replaces a
populated or malformed config. The same command lists release-embedded
templates, applies one by name, and appends an explicit target; replacement is
valid only as the deliberate `--template <name> --replace` combination.
Monorepos use explicit package targets and `cwd` values rather than recursive
guessing.

A present config that cannot load makes the pre-push test gate a violation at
the configured policy level. Absence and an intentionally empty/no-target setup
remain loud graceful no-ops. Stack detection stops emitting `structural`
blocks. Existing blocks remain schema-compatible, but doctor warns that they
are not enforced; this decision does not wire the prescriptive validator.
Schema descriptions state the runner's actual truthy `CI` rule and the public
full/essential/quick alias behavior.

## Consequences

- Release binaries can use every shipped template without a source checkout.
- Re-running setup is idempotent unless the operator explicitly requests a
  template replacement.
- Broken project-owned test intent cannot silently disable a configured push
  gate or be overwritten by repair-by-replacement.
- Multi-package commands remain a reviewed project decision, consistent with
  ADR-0003; the CLI supplies deterministic mechanics, not recursive judgment.
- Structural configuration is honest but dormant until a future decision wires
  and qualifies the validator.
