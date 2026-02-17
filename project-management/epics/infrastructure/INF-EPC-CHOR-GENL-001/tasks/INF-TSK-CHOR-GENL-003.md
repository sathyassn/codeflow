# INF-TSK-CHOR-GENL-003

## Restructure pathflow-config with task_order objects, fix PF7, add parallel batch execution

| Field | Value |
|-------|-------|
| **Task ID** | INF-TSK-CHOR-GENL-003 |
| **Epic** | INF-EPC-CHOR-GENL-001 (Ongoing Infrastructure Chores) |
| **Status** | complete |
| **Area** | INF (Infrastructure) |
| **Work Type** | CHOR (Chore) |
| **Domain** | GENL (General) |
| **Branch** | chore/pathflow-config-restructure |
| **Origin** | informal |

## Description

Restructure pathflow-config.json to add task_order objects for each phase, fix PF7-END phase definition, and add parallel batch execution support. Also update CLAUDE.md to reflect changes.

## Scope

- `pathflow-config.json`
- `CLAUDE.md`

## Progress

- **Completed:** Restructured pathflow-config.json: added phase_order to all phases, converted tasks to structured objects with task_order, fixed PF7 to 3 tasks with hooks cleanup, added max_parallel/batch_size to all stages. Updated CLAUDE.md with Parallel Batch Execution subsection, PF4/PF7 row updates. 2 commits on chore/pathflow-config-restructure.
