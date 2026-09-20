---
id: ADR-0052
title: separate cf-present ephemeral runtime from durable state
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — separate the derived browser runtime from durable session authority and tighten identity-scoped recovery
---

# ADR-0052: separate cf-present runtime from durable state

## Context

Exact review of the unintegrated TSK-011 implementation found that the browser
profile lived inside the quota-governed durable session tree even though the
browser writes it without CodeFlow's locks, capacity admission, or file-mode
contract. The same review found non-convergent Windows recovery after a leader
disappeared, incomplete process-tree rollback when launch registration failed,
post-create rather than create-time Windows privacy, replay and crash-cleanup
gaps, imprecise retention accounting, cleanup coupling between unrelated
sessions, and rejection of an ordinary relative export path. Treating these as
small closeout deviations would weaken the accepted state, safety, and cleanup
contract.

## Decision

Keep versioned session snapshots, immutable revisions, feedback events, and
transaction markers as the only durable state and project-quota authority. Put
the browser-owned profile/cache and bounded CodeFlow-owned bootstrap, ready,
and recovery controls in a derived owner-private per-project/per-session
ephemeral runtime root. That root is never a second history authority. Its
CodeFlow-owned controls have a small exact budget; browser-owned growth is
mitigated by the closed no-remote-content boundary, conservative cache/network
flags, bounded session lifetime, and identity-scoped cleanup rather than being
misrepresented as part of the durable hard quota.

Acquire locks in project-mutation, session, then runtime-control order. A
durable clear or eviction removes its derived runtime only after exact
process/resource absence proof. Unix re-proves the qualified process group.
Windows uses the existing instance and profile markers for bounded process
candidacy rather than parent-PID ancestry alone; enumeration failure, an
unreadable command line, a remaining exact candidate, or an inconclusive
profile-lock probe is ambiguity and retains recovery state without signalling
or deleting. Launch-registration failure applies the same proof to the
just-created process tree and keeps actionable evidence if rollback cannot be
proved complete.

Create Windows private files with a protected owner-only security descriptor
passed to `CreateFileW`, then verify the returned handle; no permissive inherited
ACL interval is accepted. Replay one feedback ledger so an exact accepted
receipt retry succeeds before current-revision or active-session checks, and an
identical terminal retry at its delivered or terminal version returns the
existing transition without growth. Conflicting or unrelated stale retries
remain errors.

Under the project mutation lease, reconcile only exact-name unpublished atomic
temporaries and interrupted trash whose transaction state proves safe removal.
Recompute bounded project size after each size-driven eviction. Selected clear
loads and locks only its named session; bulk operations continue across
independently isolated failures, report partial failure, and never delete
ambiguous state. Resolve an export path with no explicit parent against the
current directory while retaining create-new confinement and owner privacy.

This decision changes no product outcome, task owner, graph, or integration
target. Plan v5 adds only the explicit `update` and `resolve` commands needed
to exercise SPC-004's already accepted immutable-revision and terminal-feedback
states. It adds no daemon, database, remote viewer, generic process manager,
browser framework, or public configuration. ADR-0049 and ADR-0050 remain
historical decisions; this ADR supersedes only their same-tree runtime
assumption and post-create Windows DACL mechanism where those details conflict.

## Consequences

- Browser file churn no longer races or consumes the durable project quota, and
  durable state keeps one inspectable authority.
- Cleanup and recovery become more conservative: ambiguous native process or
  resource evidence retains state and may require an explicit operator action.
- The runtime gains a second physical root and one additional lock, so derived
  identity, lock order, cross-platform permissions, orphan cleanup, and
  crash/reboot behavior require deterministic and native qualification.
- Exact retries and independently isolated cleanup improve convergence without
  turning corruption or identity ambiguity into silent success.
- Native Windows remains a release qualification boundary; cross-compilation
  proves buildability only.

## Architecture impact

When TSK-011 lands, `docs/architecture.md` describes the durable-state and
derived-runtime roots, canonical lock order, marker-and-resource absence proof,
create-time Windows privacy, and coupled lifecycle cleanup.
