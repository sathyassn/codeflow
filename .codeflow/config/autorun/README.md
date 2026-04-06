---
title: Autorun Batch File Reference
description: Schema reference, defaults, naming conventions, and directory index for CodeFlow autorun batch files
type: reference
---

# Autorun Batch File Reference

## Table of Contents

- [Overview](#overview)
- [Batch File Schema](#batch-file-schema)
- [Autonomous Defaults](#autonomous-defaults)
- [auto_merge Rules](#auto_merge-rules)
- [Final PR](#final-pr)
- [Task Entry Schema](#task-entry-schema)
- [Naming Conventions](#naming-conventions)
- [Directory Structure](#directory-structure)
- [Example Index](#example-index)
- [Usage](#usage)

## Overview

Autorun batch files define sets of tasks to execute autonomously in parallel or sequence. Each batch file specifies task IDs, execution ordering (via `depends_on`), and optional batch-level settings.

The task markdown file at `project-management/epics/{AREA}/{AREA}-EPC-{NNN}/tasks/{task-id}.md` is the source of truth for acceptance criteria, file scope, and scope policy. The batch file specifies only execution topology and optional overrides.

## Batch File Schema

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Unique batch identifier; used in auto-generated branch names |
| `max_workers` | integer | yes | — | Maximum concurrent task workers |
| `target` | string | no | auto-generated | PR base branch (see [Autonomous Defaults](#autonomous-defaults)) |
| `auto_merge` | `boolean` | no | inferred | Whether worker PRs auto-merge after CI passes (see [auto_merge Rules](#auto_merge-rules)) |
| `final_pr` | `boolean` | no | inferred | Whether to create a final PR from the integration branch to `final_pr_base` after all workers complete (see [Final PR](#final-pr)) |
| `final_pr_base` | string | no | `"main"` | Base branch for the final PR. Only meaningful when `final_pr` is `true`. Must differ from `target`. |
| `tasks` | list | yes | — | Ordered list of task entries (see [Task Entry Schema](#task-entry-schema)) |

### Minimal Batch File

```yaml
name: my-batch
max_workers: 2
tasks:
  - id: AREA-TSK-NNN-001
  - id: AREA-TSK-NNN-002
```

## Autonomous Defaults

When `target` is omitted, the system auto-generates an integration branch:

```
autorun/{batch-name}-{session-suffix}
```

Where `{session-suffix}` is the last 8 characters of the batch session ID. This branch is created off `main` before any workers start. All worker branches are created off this integration branch.

A final summary PR from the integration branch to `main` is created by default for auto-generated targets (see [Final PR](#final-pr)). To skip it, set `final_pr: false` explicitly.

To override the target: set `target` explicitly.

| `target` value | `auto_merge` inference | `final_pr` inference |
|----------------|----------------------|---------------------|
| omitted (auto-generated) | `true` | `true` — `resolve_target()` sets both `auto_merge` and `final_pr` |
| `autorun/my-branch` (explicit) | `true` | `true` |
| `main` | `false` (forced — protected) | `false` |
| `release/v2` | `false` (forced — protected) | `false` |

## auto_merge Rules

`auto_merge` is `Option<bool>` — when omitted, the system infers it from the target branch's protection status:

| Target branch | Inferred auto_merge | Reason |
|---------------|--------------------|----|
| `main` | `false` (forced) | Protected branch — auto_merge forbidden |
| `master` | `false` (forced) | Protected branch — auto_merge forbidden |
| `release/*` | `false` (forced) | Protected branch — auto_merge forbidden |
| `production` | `false` (forced) | Protected branch — auto_merge forbidden |
| `autorun/*` | `true` | Integration branch — safe to auto-merge |
| Any other non-protected | `true` | Assumed integration branch |

Setting `auto_merge: true` with a protected target is a validation error at batch parse time.

Setting `auto_merge: false` explicitly overrides the inferred value — useful for requiring human review even on integration branches.

Worker PRs that auto-merge are serialized through the Loro CRDT merge queue (FIFO). Each worker enqueues before merge and dequeues after, preventing concurrent merge conflicts.

## Final PR

The `final_pr` flag controls whether the orchestrator creates a summary PR from the integration branch to `final_pr_base` after all workers complete.

**When `final_pr: true`:**

1. All workers run and merge their PRs to the integration branch (auto-merge if configured).
2. After all workers complete, the orchestrator creates a single final PR from the integration branch to `final_pr_base`.
3. A human reviews and merges the final PR via the GitHub UI.

This is the recommended pattern for named integration branches — it consolidates all worker changes into a single reviewed PR targeting `main` (or another base).

**Inference rules (when `final_pr` is omitted):**

| Condition | Inferred `final_pr` | Rationale |
|-----------|---------------------|-----------|
| `target` is omitted (auto-generated) | `true` | `resolve_target()` sets `final_pr = true` alongside `auto_merge = true` |
| `target` is `"main"` or `"master"` | `false` | Workers target main directly — no integration branch to PR from |
| `target` is any other explicit branch | `true` | Explicit non-main target implies integration branch workflow |

**`final_pr_base` field:**

- Default: `"main"` when omitted or empty.
- Must differ from `target` (you cannot PR from a branch to itself).
- Use `final_pr_base: develop` for teams using a `develop` → `main` workflow.
- `final_pr_base` is ignored when `final_pr` is `false`.

**Example — explicit integration branch with final PR:**

```yaml
name: sprint-42
max_workers: 3
target: autorun/sprint-42
auto_merge: true
final_pr: true
final_pr_base: main
tasks:
  - id: FRT-TSK-001-001
  - id: FRT-TSK-001-002
```

**Example — auto-generated target with final PR suppressed:**

```yaml
name: my-batch
max_workers: 2
final_pr: false         # explicit override — auto-generated targets infer final_pr: true by default
tasks:
  - id: AREA-TSK-NNN-001
  - id: AREA-TSK-NNN-002
```

## Task Entry Schema

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Task ID matching a markdown file in `project-management/epics/` |
| `depends_on` | list | no | Task IDs that must complete before this task starts |
| `scope_policy` | string | no | Override the task markdown's `scope_policy` — can only TIGHTEN (soft → hard), never loosen; `permissive` is forbidden for autorun tasks |

### Task Requirements (enforced at runtime)

Each `id` must have a corresponding markdown file at:

```
project-management/epics/{AREA}/{AREA}-EPC-{NNN}/tasks/{task-id}.md
```

The task markdown must contain frontmatter with:

```yaml
autorun_eligible: true       # marks task as safe for autonomous execution
acceptance: [list]           # at least one acceptance criterion
file_scope: [list]           # file paths this task may modify
scope_policy: soft or hard   # NOT permissive (forbidden for autorun)
```

## Naming Conventions

| Entity | Convention | Example |
|--------|-----------|---------|
| Batch file | `{area}-{epic-slug}-{topic}.yaml` | `inf-epc-024-retention-policies.yaml` |
| Batch `name` field | kebab-case, matches filename stem | `inf-epc-024-retention-policies` |
| Auto-generated branch | `autorun/{batch-name}-{session-suffix}` | `autorun/inf-epc-024-retention-policies-a1b2` |
| Worker branch | `feat/{task-id}` or `fix/{task-id}` | `feat/inf-tsk-024-016` |

## Directory Structure

```
.codeflow/config/autorun/
├── README.md               ← this file
├── batches/                ← production batch files for real epics
│   ├── inf-epc-024-phase1-audits.yaml
│   └── inf-epc-024-retention-policies.yaml
├── examples/               ← reference examples for batch patterns
│   ├── minimal-autonomous.yaml         ← recommended default (no target/auto_merge)
│   ├── simple-sequential.yaml          ← two tasks, one after the other
│   ├── complex-dependencies.yaml       ← diamond DAG with scope_policy override
│   ├── custom-integration.yaml         ← explicit target + final_pr + auto_merge: true
│   ├── manual-review.yaml              ← explicit auto_merge: false for human review
│   ├── direct-to-main.yaml             ← target: main for direct PRs
│   └── final-pr-to-develop.yaml        ← integration branch with final_pr_base: develop
└── local/                  ← gitignored local batch files (developer use)
```

## Example Index

| File | Pattern | When to Use |
|------|---------|-------------|
| `examples/minimal-autonomous.yaml` | No target, no auto_merge | Most batches — system selects correct defaults |
| `examples/simple-sequential.yaml` | Sequential dependency | Task B must run after Task A |
| `examples/complex-dependencies.yaml` | Diamond DAG + scope override | Multi-level dependencies with policy tightening |
| `examples/custom-integration.yaml` | Explicit `target` + `final_pr` + `auto_merge: true` | Named integration branch with summary PR to main |
| `examples/manual-review.yaml` | Explicit `auto_merge: false` | Sensitive work requiring human sign-off on each PR |
| `examples/direct-to-main.yaml` | `target: main` | Small batches with standalone reviewable PRs |
| `examples/final-pr-to-develop.yaml` | `final_pr_base: develop` | Teams using develop → main workflow |

## Usage

```bash
# Run a batch
codeflow autorun run --batch .codeflow/config/autorun/batches/inf-epc-024-retention-policies.yaml

# List available batches
codeflow autorun batches

# Monitor running batch
codeflow autorun status --watch

# Resume non-completed tasks from a previous batch run
codeflow autorun resume
```
