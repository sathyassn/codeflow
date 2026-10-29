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

- **Baseline:** the line's rule. It applies where the harness configuration the guard reads shows none of the triggers below. That is a reading of files, not proof that a sandbox contains the call.
- **Tightened:** the baseline plus a strict rule that exempts no program by name. It applies on any trigger: native Windows under Claude Code or Grok Build; a disabled, fail-open or unset sandbox; a missing floor entry; an allow entry at a protected path, a startup file or a git configuration file; an `allowRead` entry beyond the shipped ones; any `excludedCommands`; an escape setting, including `allowAppleEvents` and `enableWeakerNetworkIsolation`; a file that cannot be read, is not a regular file or is too large; a Codex profile that grants more than the shipped profile, drops one of its denies or widens its network, or an approval policy other than `never`; a Grok profile missing a shipped entry or deny; a Codex or Grok call with no configuration of its harness; a Claude call whose hook environment names no session project; a linked worktree whose main checkout cannot be found; a call that asks to run outside the sandbox (`dangerouslyDisableSandbox`); or `security.text_guard: strict`.

What is read: for a Claude call, the session project Claude Code exports as `CLAUDE_PROJECT_DIR`; for any call, the repository of its working directory, each directory between them that holds harness settings, and the main checkout of a linked worktree. One loosened project tightens the call.

The tightened outcome keeps every baseline finding. Neither outcome allows what the line refuses, with one exception: a line made only of bare `cd`, `pushd` and `popd` builtins writes nothing, so the baseline allows it (H-3, issue 342) and the tightened outcome still refuses it. A replay of the historical corpus is the test, and it checks that exception as a class.

Harnesses are identified by a union rule: only a key unique to one harness excludes another.

This amends ADR-0008 (one rule for every harness becomes one baseline with a configuration-selected addition) and ADR-0029 (a retry the harness marks is judged under the strict rule).

## Consequences

- On the common posture, Claude Code on macOS or Linux with the shipped preset, behaviour is unchanged.
- This is a breaking change (operator, 2026-10-28; release 4.0.0). A tightened host refuses 42 of the 49 everyday lines where the line refuses 2, until its settings are repaired. `codeflow doctor --check sandbox` names each trigger per harness; native Windows under Claude Code has no remedy.
- What no file shows is not read: a `--settings` file, server-managed settings, MDM policy, a Codex launch flag, the session of a call whose harness is not identified, and a retry the harness does not mark. Those calls get the baseline today. Failing closed there is tracked as TSK-1052.
- Qualification, receipts and the relaxation of text rules on an attested host are 4.1.0 work (EPC-023, TSK-1054 to TSK-1060). This decision does not provide them.

## Architecture impact

`docs/architecture.md`: the Enforcement row names `hooks/sandbox_state.rs`, which selects the exec guard's text rule from the readable harness configuration.
