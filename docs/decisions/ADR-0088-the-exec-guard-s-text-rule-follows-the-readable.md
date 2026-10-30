---
id: ADR-0088
uid: 4a97954c-7678-4aff-911e-8fc2a02dcb78
title: "The exec guard's text rule is strict unless the operator relaxes it"
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

# ADR-0088: The exec guard's text rule is strict unless the operator relaxes it

## Context

A text filter cannot contain a program it does not read. The PR 331 reviews found 14 classes of command that pass any rule the guard can apply to shell text. The strict rule that refuses them also refuses 42 lines of a 49-line everyday sample. Where the OS sandbox is enabled and fails closed with the shipped deny entries, the kernel already denies writes to the enforcement set; where it is absent, loosened or bypassed, nothing else stands behind the text. ADR-0008 made the exec guard one rule for every harness. ADR-0029 let a sandbox failure request one classified unsandboxed retry.

Two designs chose the rule from the harness configuration the guard can read: first a list of loosening settings, then an attestation that each layer equals the shipped preset or a narrower form. Each review round from 15 to 18 found a layer or key the reader missed, in configuration formats CodeFlow does not own, so a weaker configuration still selected the relaxed rule. A feature that cannot be made sound is removed rather than patched (operator rule of 2026-10-24).

## Decision

The exec guard applies one of two outcomes per call (operator decision of 2026-10-29, option 2, TSK-397):

- **Strict**, the default: the line's rule plus a strict rule that exempts no program by name. It applies to every call unless the operator chose relaxed.
- **Relaxed:** the line's rule. It applies only when `security.text_guard` is `relaxed` in every policy source the hook reads, each read from the remote-tracking refs as last fetched: the call's repository (its default branch and any declared line, where `strict` wins) and, for a Claude Code call, the session's project. Each source must be a plain ref under `refs/remotes/<remote>/`, the remote's `HEAD` must point inside that namespace at every step, and no loose ref file, or directory on its way, may be a symbolic link. A working copy, a local branch or tag and a repository without a remote never relax the rule. Relaxed is the operator's choice; the guard verifies no sandbox.
- **What relaxed trusts:** the local remote-tracking state, which is not authenticated. A process that can write the repository's `.git` directory can point a remote-tracking ref at a local relaxed commit, and a stale ref keeps an earlier relaxed choice until the next fetch. That write is the same capability that already defeats any text rule (it can write a script the guard never reads), so a forged ref grants nothing new under this ADR's threat model, where the OS sandbox is the boundary. Authenticating the refs is the local baseline work of TSK-1057.
- **Enforcement:** the strict rule's own refusals always block. `git.hook_integrity` sets the level of the line's findings only, so a legacy `warn` or `off` never turns the strict rule into advice.

Under relaxed, two facts the guard reads still select the strict rule: a recognized marked retry (`dangerouslyDisableSandbox` with any value but absent, `null` or `false`), and native Windows, where Claude Code and Grok Build have no sandbox (the guard does not establish which harness made a call, so every native Windows call is strict).

The strict outcome keeps every relaxed finding. Neither outcome allows what the line refuses, with one exception: a line made only of bare `cd`, `pushd` and `popd` builtins writes nothing, so relaxed allows it (H-3, issue 342) and strict still refuses it. A replay of the historical corpus is the test, and it checks that exception as a class.

This amends ADR-0008 (one rule for every harness becomes a strict default with an operator-chosen relaxation) and ADR-0029 (a retry the harness marks is judged under the strict rule).

## Consequences

- This is a breaking change (operator, 2026-10-28; release 4.0.0). Every project is strict until its operator lands `relaxed`: on the everyday sample, 42 of 49 lines refuse where the line refused 2, among them `echo "$HOME"`, `git commit -m "$MSG"` and `[ -f "$f" ]`.
- `codeflow init` and `codeflow update` never write the key. `codeflow doctor --check sandbox` prints the rule in force and, for relaxed, the ref it was read from, as "relaxed by operator choice, not verified", says what that ref is, and names the level each kind of finding takes.
- Relaxed takes effect only once it is on every protected ref the hook reads. For CodeFlow's own repository, which reads `main` and the integration line, that is when it reaches `main` through the checkpoint.
- A relaxed project trusts the OS sandbox it configures and the remote-tracking refs as last fetched; the guard does not check that the sandbox is enabled, fails closed or carries the floor. An unsandboxed retry the harness does not mark (Codex has no such field) follows the relaxed rule.
- A relaxed rule tied to verified containment belongs to the 4.1.0 qualification work (EPC-023, TSK-1054).

## Architecture impact

`docs/architecture.md`: the Enforcement row names `hooks/sandbox_state.rs`, which selects the exec guard's text rule from the landed policy, the retry flag and the platform.
