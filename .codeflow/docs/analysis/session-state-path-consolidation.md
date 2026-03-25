# Session State Path Consolidation

> **Status: COMPLETED** — The path consolidation described here was implemented
> in the Rust CLI (INF-EPC-021, PR #133). The layout below matches the current
> implementation. The Go CLI and its separate `.state/checkpoints/` directory
> have been retired.

## Problem

Session-scoped ephemeral state is spread across 3 directories:

| Directory | Contents | Purpose |
|-----------|----------|---------|
| `.state/session/{SID}/` | `is-pathflow-active` flag | PathFlow lifecycle flag |
| `.state/checkpoints/pathflow/{SID}/` | `phase-tasks.json` | Phase checkpoint tracking |
| `.state/sentinels/pathflow/{SID}/` | `pathflow-*` files | Gate enforcement sentinels |

The flag and checkpoint are produced/consumed by the same lifecycle hooks (session-start, session-end, checkpoint hooks) but live in separate directory trees.

## Decision

Consolidate by concern:

- **Flag + checkpoint** together under `.state/session/{SID}/pathflow/` (same lifecycle, same producers/consumers)
- **Sentinels** stay at `.state/sentinels/pathflow/{SID}/` (consumed by gate hook PreToolUse hot path -- needs direct predictable path)

## New layout

```text
.state/session/{SID}/pathflow/
    is-pathflow-active        (moved from .state/session/{SID}/)
    pathflow-phase-tasks.json (moved from .state/checkpoints/pathflow/{SID}/)

.state/sentinels/pathflow/{SID}/   (unchanged)
    pathflow-pf-1
    pathflow-pf-3
    pathflow-ws-dev
    ...
```

## Rationale

1. **Sentinels in hot path**: The `cf-pre-tool-use-pathflow-gate.sh` hook checks sentinel existence on every Edit/Write/Bash call. A predictable, flat path avoids indirection.
2. **Flag + checkpoint co-located**: Both are produced by session-start/checkpoint hooks and consumed by session-end cleanup. Grouping them under one directory simplifies lifecycle management.
3. **Consistent flag name**: `is-pathflow-active` retains the existing name, now co-located under `pathflow/` directory.
4. **Eliminates `.state/checkpoints/`**: This directory tree is no longer needed. A `.gitignore` entry prevents stale data accumulation.

## Impact

~13 files updated: pathflow-state library, context-lib, sentinel library, 6 hook scripts, CLAUDE.md, analysis doc, .gitignore, and 3 test files.
