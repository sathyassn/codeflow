---
id: ADR-0088
uid: 4a97954c-7678-4aff-911e-8fc2a02dcb78
title: "The exec guard's text rule follows the readable sandbox configuration"
date: 2026-10-28
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "the Enforcement row names `hooks/sandbox_state.rs`, which selects the exec guard's text rule"
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0088: The exec guard's text rule follows the readable sandbox configuration

## Context

A text filter cannot contain a program it does not read. The PR 331 reviews found 14 classes of command that pass any rule the guard can apply to shell text. The strict rule that refuses them also refuses about 42 of 49 everyday lines. Where the OS sandbox is enabled, fails closed and carries the shipped floor, the kernel already denies writes to the enforcement set. Where it is absent, loosened or bypassed, nothing else stands behind the text. ADR-0008 made the exec guard one rule for every harness. ADR-0029 let a sandbox failure request one classified unsandboxed retry.

## Decision

The exec guard applies one of two outcomes per call (operator decision of 2026-10-28, TSK-397 design D Part A):

- **Baseline:** the line's rule. It applies where the readable harness configuration shows the sandbox enabled, failing closed and carrying the shipped floor.
- **Tightened:** the baseline plus a strict rule that exempts no program by name. It applies on any trigger: native Windows under Claude Code or Grok Build; a disabled, fail-open or unreadable configuration; a missing floor entry; an allow entry at a protected path; any `excludedCommands`; an escape setting; a Codex profile outside `cf-guard` and `cf-builder`, or an approval policy other than `never`; a Grok profile missing a shipped entry; a call that asks to run outside the sandbox (`dangerouslyDisableSandbox`); or `security.text_guard: strict`.

The tightened outcome keeps every baseline finding, so neither outcome allows what the line refuses. A replay of the historical corpus is the test.

Harnesses are identified by a union rule: only a key unique to one harness excludes another.

This amends ADR-0008 (one rule for every harness becomes one baseline with a configuration-selected addition) and ADR-0029 (a retry the harness marks is judged under the strict rule).

## Consequences

- On the common posture, Claude Code on macOS or Linux with the shipped preset, behaviour is unchanged.
- A host whose settings drifted from the floor, or that lists any `excludedCommands`, refuses about 42 of 49 everyday lines where the line refused 5, until its settings are repaired. `codeflow doctor --check sandbox` names the trigger.
- What no file shows is not read: a `--settings` file, server-managed settings, MDM policy, a Codex launch flag, and a retry the harness does not mark. Those calls get the baseline. TSK-1052 makes those cases fail closed.
- Qualification, receipts and the relaxation of text rules on an attested host are 4.1.0 work (EPC-023, TSK-1054 to TSK-1060). This decision does not provide them.

## Architecture impact

`docs/architecture.md`: the Enforcement row names `hooks/sandbox_state.rs`, which selects the exec guard's text rule from the readable harness configuration.
