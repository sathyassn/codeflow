# INF-TSK-CHOR-GENL-002: Fix pre-PR memory update flow in PF6-COMPLETE

## Metadata

| Field | Value |
|-------|-------|
| Task ID | INF-TSK-CHOR-GENL-002 |
| Epic | INF-EPC-CHOR-GENL-001 |
| Status | in_progress |
| Area | INF (Infrastructure) |
| Work Type | CHOR (Chore) |
| Domain | GENL |
| Branch | chore/fix-pf6-memory-flow |
| Priority | normal |
| Origin | informal |

## Description

Fix the PF6-COMPLETE phase to ensure cf-knowledge-layer updates memory/DB state before cf-git-operations creates the PR. Currently the ordering may not guarantee that memory updates happen pre-PR.

## Scope

- `pathflow-config.json` - Phase configuration
- `cf-git-operations.md` - Agent definition for git operations
- `CLAUDE.md` - Team lead instructions

## Progress

- Started: 2026-02-16
