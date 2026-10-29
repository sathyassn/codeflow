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

A text filter cannot contain a program it does not read. The PR 331 reviews found 14 classes of command that pass any rule the guard can apply to shell text. The strict rule that refuses them also refuses 42 lines of a 49-line everyday sample. Where the OS sandbox is enabled and fails closed with the shipped deny entries, the kernel already denies writes to the enforcement set. Where it is absent, loosened or bypassed, nothing else stands behind the text. ADR-0008 made the exec guard one rule for every harness. ADR-0029 let a sandbox failure request one classified unsandboxed retry.

## Decision

The exec guard applies one of two outcomes per call (operator decision of 2026-10-28, TSK-397 design D Part A):

- **Baseline:** the line's rule. It applies only when the guard attests the configuration the call's harness runs under: the payload identifies one harness, every configuration layer that harness reads is read, and each layer's containment settings equal the shipped preset or a narrower form the guard lists. That is a reading of files, not proof that a sandbox contains the call.
- **Tightened:** the baseline plus a strict rule that exempts no program by name. It applies whenever the configuration is not attested: unreadable, unknown, ambiguous or customised beyond a listed narrower form. It also applies on native Windows under Claude Code or Grok Build, on a call that asks to run outside the sandbox (`dangerouslyDisableSandbox`), and when the policy of the call's repository or of the Claude session's project sets `security.text_guard: strict` or cannot be read.

What is attested:

- Claude Code: the session project (`CLAUDE_PROJECT_DIR`, required), its repository root and a linked worktree's main checkout, each with shared and local settings, the user settings and the managed settings. In each layer, `sandbox` and `permissions` may hold only listed keys, with the shipped value or a narrower one: more denies, a subset of the shipped `allowRead` and `allowWrite`, no `excludedCommands`, no additional directories. Together the layers must enable a fail-closed sandbox and carry the floor. Managed preferences and a cached server-managed file are not attested.
- Codex: the user configuration, profile files beside it, and the project configuration when the user configuration trusts the project. Every top-level key must be a known one; containment keys must hold shipped values (`approval_policy = "never"`, a shipped `default_permissions`, the network proxy and hooks on); a shipped permission profile may only add denies or narrow an access. System and managed files are not attested.
- Grok Build: `GROK_SANDBOX` or the user configuration's `[sandbox] profile` must select a shipped profile, and its definition in the user and project profile files must equal the shipped one or narrow it.

An earlier revision of this decision enumerated loosening settings. Three review rounds each found a loosening it lacked, in configuration formats CodeFlow does not own, so the list was replaced by this attestation (Codex 17, Fable 02).

The tightened outcome keeps every baseline finding. Neither outcome allows what the line refuses, with one exception: a line made only of bare `cd`, `pushd` and `popd` builtins writes nothing, so the baseline allows it (H-3, issue 342) and the tightened outcome still refuses it. A replay of the historical corpus is the test, and it checks that exception as a class.

A payload is identified by keys unique to one harness; a payload with none, or with the keys of several, is not attested.

This amends ADR-0008 (one rule for every harness becomes one baseline with a configuration-selected addition) and ADR-0029 (a retry the harness marks is judged under the strict rule).

## Consequences

- On the common posture, Claude Code on macOS or Linux with the shipped preset, behaviour is unchanged.
- This is a breaking change (operator, 2026-10-28; release 4.0.0). A tightened host refuses 42 of the 49 everyday lines where the line refuses 2, until its settings are repaired. `codeflow doctor --check sandbox` names each reason per harness and what to change; native Windows under Claude Code has no remedy.
- The cost of attestation is over-refusal: any customisation beyond a listed narrower form selects the strict rule, even one that loosens nothing. A Codex seat needs its project trusted in the user configuration and the shipped containment values in force; a Grok seat needs its profile selected through `GROK_SANDBOX` or the user configuration, since a `--sandbox` launch flag is not visible to the guard.
- Not attested: Claude Code top-level keys outside `sandbox` and `permissions` (such as `env`), which configure no documented containment. What no file shows is not read: launch flags (`--settings`, `--sandbox`, `-c`, `--profile`), server-managed settings that are not cached on disk, and a retry the harness does not mark. Those calls can get the baseline today. Failing closed there is tracked as TSK-1052.
- Qualification, receipts and the relaxation of text rules on an attested host are 4.1.0 work (EPC-023, TSK-1054 to TSK-1060). This decision does not provide them.

## Architecture impact

`docs/architecture.md`: the Enforcement row names `hooks/sandbox_state.rs`, which selects the exec guard's text rule from the readable harness configuration.
