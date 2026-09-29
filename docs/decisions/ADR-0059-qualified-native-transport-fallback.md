---
id: ADR-0059
uid: 36924baa-ed0d-4e39-b567-8ccf3a0698ae
title: qualify native transport by capabilities instead of plugin exclusivity
date: 2026-09-10
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0059 — qualified native transport fallback

## Context

The official Claude-to-Codex plugin remains preferred, but its state-directory
and nested-sandbox incompatibilities can prevent an otherwise available native
Codex session from starting. Independent reports describe
[plugin state writes](https://github.com/openai/codex-plugin-cc/issues/603) and
[nested sandboxes](https://github.com/openai/codex-plugin-cc/issues/167), including
a macOS reproduction in the latter's comments. These are environment-specific
reports, not proof of a universal outage; their proposed unrestricted-sandbox
workarounds are not adopted.

## Decision

Supersede only the plugin-exclusive Claude-to-Codex choice in ADR-0018,
ADR-0023 and subsequent references. Prefer the official plugin; permit an
official native App/interactive CLI fallback after a bounded qualification of
the actual tools, authority and five evidence obligations. Preserve native
independence, requested/observed identity honesty, read-only or worktree scope,
Git/secret controls, bounded failure, result verification and cleanup. No
headless peer, hand-rolled RPC or same-lineage impersonation is introduced.

Sandbox nesting is not itself an acceptance criterion. Both launcher and child
need an evidenced effective boundary; a child sandbox cannot protect its
launcher. Supported narrow configuration must remain within explicit authority,
and alternative clients cannot evade the intent of a safety denial. No global
permission change, broad shell exclusion or plugin fork is needed by this
decision. Anthropic documents protected paths and incompatible-tool handling
in its [sandbox guidance](https://code.claude.com/docs/en/sandboxing).

Independent native product trials need not each start a nested duo unless the
case tests delegation. Their host supplies cross-lineage review. A changed
envelope is a new declared cohort, retaining prior records and applicable
tasks/oracles; product acceptance does not claim qualification of an unrun
transport or model binding.

## Consequences

One failed optional integration no longer holds unrelated product verification
hostage. The cost is qualifying the alternative's actual capabilities once,
and rechecking materially changed boundaries. Missing required tools, behavior
or independent review remains a gap. Frozen failures stay visible. This adds
guidance, not a process broker, private harness-state reader or security bypass.
`doctor` still reports absence of the preferred plugin as a prerequisite gap;
fallback qualification lives in run evidence, not in that inventory check.
