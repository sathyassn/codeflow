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
| `auto_merge` | boolean | no | inferred | Auto-merge worker PRs after CI passes (see [auto_merge Rules](#auto_merge-rules)) |
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

This branch is created off `main` before any workers start. All worker branches are created off this integration branch. After all workers complete, a single human-reviewed summary PR merges the integration branch to `main`.

To override: set `target` explicitly.

| `target` value | Effect |
|----------------|--------|
| omitted | Auto-generated `autorun/{name}-{suffix}` integration branch |
| `autorun/my-branch` | Explicit integration branch (pre-existing or created by orchestrator) |
| `main` | Direct-to-main pattern (protected — auto_merge forbidden) |
| `release/v2` | Release branch target (protected — auto_merge forbidden) |

## auto_merge Rules

When `auto_merge` is omitted, the system infers it from `target`:

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
│   ├── minimal-autonomous.yaml     ← recommended default (no target/auto_merge)
│   ├── simple-sequential.yaml      ← two tasks, one after the other
│   ├── complex-dependencies.yaml   ← diamond DAG with scope_policy override
│   ├── custom-integration.yaml     ← explicit target + auto_merge: true
│   ├── manual-review.yaml          ← explicit auto_merge: false for human review
│   └── direct-to-main.yaml         ← target: main for direct PRs
└── local/                  ← gitignored local batch files (developer use)
```

## Example Index

| File | Pattern | When to Use |
|------|---------|-------------|
| `examples/minimal-autonomous.yaml` | No target, no auto_merge | Most batches — system selects correct defaults |
| `examples/simple-sequential.yaml` | Sequential dependency | Task B must run after Task A |
| `examples/complex-dependencies.yaml` | Diamond DAG + scope override | Multi-level dependencies with policy tightening |
| `examples/custom-integration.yaml` | Explicit `target` + `auto_merge: true` | Named, persistent integration branch |
| `examples/manual-review.yaml` | Explicit `auto_merge: false` | Sensitive work requiring human sign-off on each PR |
| `examples/direct-to-main.yaml` | `target: main` | Small batches with standalone reviewable PRs |

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
