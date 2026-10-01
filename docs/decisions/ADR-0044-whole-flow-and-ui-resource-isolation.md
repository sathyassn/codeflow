---
id: ADR-0044
uid: 99f2ed55-f45b-403a-8354-08005302a7f6
title: require whole-flow evidence and isolated UI resources
date: 2026-07-26
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — verification planning gains an affected-journey topology and task-owned browser-resource lease, while existing worktree evidence gains closeout cadence
---

# ADR-0044: whole-flow and UI resource isolation

## Context

CodeFlow already requires unit, integration, end-to-end, security, and rendered
UI evidence. It also permits routine Playwright work to run headless and
requires safe post-merge worktree cleanup. Three ambiguities remain:

1. a collection of passing layer tests can be reported as end-to-end even when
   no run crosses the real changed frontend, service, persistence, runtime, or
   infrastructure boundaries;
2. concurrent browser workers in one workspace can collide through
   Playwright MCP's default persistent profile, application or service ports,
   shared accounts/data, and artifact paths; and
3. safe cleanup rules do not themselves ensure proven-landed worktrees are
   closed promptly, so correct but stale worktrees accumulate.

The fix must stay stack- and harness-neutral. CodeFlow should not impose one
port range, test database scheme, browser transport, or generic deletion
command on every consuming project.

## Decision

For every material changed journey, Plan vN records an affected topology:
entry point; each changed in-project component and trust/state boundary;
relevant deployment/runtime wiring; observable outcome; and recovery behavior.
At least one end-to-end run at the highest faithful surface traverses every
applicable affected boundary. Unit and integration tests remain mandatory
diagnostics but do not substitute for the vertical run.

A double is allowed beyond an uncontrolled external boundary when the real
dependency is unsafe, unavailable, non-deterministic, or disproportionate.
That seam is disclosed, contract-checked where feasible, and never described
as exercised. A changed in-project boundary is not mocked in the evidence
claimed as whole-flow proof. Infrastructure, packaging, installer, migration,
and configuration changes add an ephemeral or native-runtime canary.

Every concurrent UI task receives a resource lease owned by its task/run:

```text
owner/run
├── isolated browser context/profile
├── MCP endpoint, only when the transport listens
├── non-overlapping loopback application/service endpoints
├── namespaced test state/account/schema, with worker identity when parallel
├── task-scoped artifacts/reports/traces
└── readiness, retention, teardown, and release verification
```

The consuming project defines its allocator or safe range, namespace format,
artifact root, retention, and teardown commands during customization. Routine
runs remain headless. A materially necessary headed run launches a test-owned
browser/profile; neither Playwright nor Computer Use may attach to or take over
the operator's existing browser, tabs, profile, or active desktop.

At orientation and post-landing closeout, the agent inventories task-owned
worktrees. Proven-landed clean entries are removed promptly using the existing
normal/squash proof rules. Active, dirty, or unproven entries are retained with
an owner, reason, and recheck event. `git worktree prune` may clean stale
administrative records after inspection, but is not merge proof.

`codeflow status` supplies the read-only local inventory for linked worktrees
and unattached local branches. It reports `removable` only when the resource is
clean and its revision is in the locally known target by ancestry or
patch-equivalent squash evidence; dirty and unproven resources are retained.
The report never deletes and does not infer harness/session ownership, so owner
clearance remains required before acting on a removable result.

Behavioral cases and deterministic contract tests pin whole-flow refusal,
parallel UI isolation/teardown, and mixed worktree-inventory disposition.

## Consequences

- End-to-end means one observed vertical journey rather than a label applied to
  disconnected layer tests.
- Parallel browser work can remain useful without cross-task state, port, or
  evidence corruption.
- Headed verification remains available without commandeering the operator's
  working browser or desktop.
- Landed worktrees stop accumulating, while dirty and unproven work remain
  protected.
- Consuming projects add a small testing-resource decision during
  customization; CodeFlow gains no universal test runner, port broker, browser
  daemon, or deletion command.

## Rejected

- **Require a live third party in every end-to-end run.** This is often unsafe
  or non-deterministic; explicit controlled seams plus contract evidence are
  more honest.
- **Assign universal ports and directories.** They conflict with project,
  platform, CI, container, and harness conventions.
- **Share the default Playwright workspace profile.** The official MCP warns
  that one persistent profile cannot serve concurrent clients safely.
- **Attach to the operator's browser for convenient authentication.** It mixes
  private state with test work and can hijack active interaction.
- **Add a generic deletion command or age-based sweeper.** Merge and ownership
  evidence, not time, decides what is safe to remove; a read-only classifier
  improves closeout without silently taking ownership.

## References

- [Playwright MCP — profiles and parallel clients](https://github.com/microsoft/playwright-mcp)
- [Playwright — test isolation and parallel workers](https://playwright.dev/docs/test-parallel)
- [Playwright — fixtures and teardown](https://playwright.dev/docs/test-fixtures)
- [Playwright — web-server lifecycle](https://playwright.dev/docs/test-webserver)
- [Playwright — authentication state per worker](https://playwright.dev/docs/auth)
