---
id: ADR-0021
title: reject changed-file coverage without a comparison base
date: 2026-07-14
status: accepted
superseded_by: null
architecture_impact: The test-config contract no longer accepts changed_files coverage rules until the gate has an explicit comparison-base input; aggregate scopes remain supported and are verdict-bearing.
---

# ADR-0021 — reject changed-file coverage without a comparison base

## Context

`codeflow test` accepted `changed_files` coverage rules but had no comparison
base from which to compute the changed set. The shared gate therefore supplied
an empty set, making the rule vacuous and allowing an enforcing configuration
to pass without evaluating any file. Inferring a base such as `HEAD~1` would be
unstable across standalone, pre-push, and integrate callers.

## Decision

Test configuration loading rejects every `changed_files` coverage rule with a
validation error until the test-gate contract provides an explicit comparison
base. The shipped schema and example omit that scope. `global`, `per_package`,
and `per_module` remain supported; their aggregate evaluator contributes to the
same threshold results and gate verdict as `per_file`.

## Consequences

- Existing configurations using `changed_files` must migrate to `per_file`,
  `global`, `per_package`, or `per_module` before upgrading.
- A coverage requirement can no longer pass merely because its input set was
  unavailable; configuration fails before tests run.
- A future changed-file implementation must first add an explicit base to the
  gate/config contract, normalize its paths, and test every caller.

## Architecture impact

The test-gate configuration boundary now excludes a scope it cannot evaluate.
Aggregate coverage scopes are wired into the existing verdict path; no module
ownership or dependency boundary changes.
