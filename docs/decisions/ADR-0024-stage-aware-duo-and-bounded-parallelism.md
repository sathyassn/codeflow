---
id: ADR-0024
title: stage-aware duo default with bounded parallel worktrees
date: 2026-07-16
status: accepted
superseded_by: null
architecture_impact: the scaffold contract routes every non-trivial repository task through one stage-aware Claude+Codex entry point and adds an explicit bounded-parallel integration contract
---

# ADR-0024 — stage-aware duo and bounded parallelism

## Context

ADR-0023 made the Claude+Codex duo host-neutral, but described it primarily as a
development flow. That leaves research, analysis, planning, review, security,
and substantive documentation open to inconsistent entry points even though
they need the same independent evidence and reconciliation. It also says that
the two seats research in parallel without defining safe parallel
implementation ownership or integrated evidence.

Unbounded fan-out is not a quality feature. Multiple agents sharing a worktree
can overwrite one another; parallel heavyweight builds and browsers can exhaust
memory; and green task branches do not establish that their combined diff is
green. Conversely, serializing independent work by default wastes both model
and host capacity.

## Decision

Make `/cf-model-orchestrator` the required entry point for every non-trivial
repository task in standard and full scaffolds. Non-trivial includes material
research, analysis, planning, design, implementation, debugging, security,
review, verification, and documentation. A conversational answer or one
obvious local edit may bypass the duo. When uncertain, use the duo.

The orchestrator selects a stage set before work begins:

- research/analysis settles evidenced findings without inventing code work;
- plan/design produces a versioned dual-approved plan and detailed tasks, then
  stops;
- implementation continues through Codex implementation and first
  verification, Claude final review, and joint closeout;
- review/verification gives both seats independent inspection authority but
  not implicit edit authority;
- substantive documentation chooses the analysis or implementation path based
  on whether repository files will change.

Keep the fixed roles from ADR-0023. Use the latest available Fable-class Claude
model at xhigh effort for reasoning, synthesis, design, coordination, and final
review. Fable may delegate mechanistic browser, Playwright, Computer Use, or MCP
operation to current Opus-class subagents, but Fable interprets the evidence and
owns the judgment. Use the strongest supported Codex coding model at xhigh for
orchestration, implementation, and difficult verification. Record actual model
versions per run; durable doctrine names routing classes rather than pins.

Parallelize only independent workstreams that shorten the critical path. Every
implementation task has one owner, branch, and worktree. Shared schemas,
lockfiles, generated registries, and conflict-prone files have one owner or run
serially. The host records the dependency graph, merge order, resource budget,
task gates, and post-merge gates, then caps concurrency from observed memory,
CPU, disk, context, browser, and tool capacity. Reduce fan-out before swap
pressure, duplicate heavy builds, or context dilution degrades the evidence.

Land task branches serially through `codeflow integrate` onto
`integration/<epic>`. Re-run affected gates after each landing and aggregate
gates on the final combined diff. Never use concurrent writers in one worktree
or rebase a shared integration branch.

Minimal remains the enforcement-only tier. It does not advertise skills that it
does not install.

## Consequences

- Harness choice no longer changes which non-trivial work receives independent
  Claude and Codex reasoning.
- Planning-only and research-only requests stop cleanly before implementation.
- Parallelism is available without treating task-local success as integrated
  proof.
- The method adds coordination overhead only where the task is non-trivial;
  small obvious edits retain a direct path.
- A missing vendor seat degrades visibly to the existing solo flow after
  preflight rather than silently simulating a duo.

## Architecture impact

The standard/full `AGENTS.md`, `CLAUDE.md`, `cf-method`, `cf-plan`, and
`cf-model-orchestrator` assets share one entry-point rule and one stage model.
The shared quality resource owns the branch/worktree/resource/integration
contract. The core binary does not become a model router.


## Note (2026-09-25)

ADR-0069 moves concrete models into the managed catalog. The reasoning,
synthesis, design, coordination and final review duties above belong to seat
`claude-primary`, whose first line owns design, and the coding duties to seat
`codex-primary`; `codeflow models resolve` returns the pinned id and effort
for each duty. Seats enter at high under ADR-0056, so the xhigh default here
stays amended as ADR-0028 and ADR-0055 record. The Fable and Opus classes
named above describe the roster at the time. The rest of this decision stands.
