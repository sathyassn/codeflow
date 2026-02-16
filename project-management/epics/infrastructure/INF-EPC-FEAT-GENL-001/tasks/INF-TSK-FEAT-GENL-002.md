---
id: INF-TSK-FEAT-GENL-002
epic_id: INF-EPC-FEAT-GENL-001
title: "Add task tracker mirroring to pathflow-config"
description: Add task_tracker templates to pathflow-config.json that mirror Claude Code internal task tracker (TaskCreate/TaskUpdate/TaskList) into the WorkGraph. Ensures JSONL/SQLite data model is primary source of truth.
status: todo
area_type: INF
work_type: FEAT
domain: GENL
origin: planned
file_scope: ["pathflow-config.json", "CLAUDE.md", "cf-knowledge-layer.md"]
scope_policy: soft
priority: normal
branch: feat/task-tracker-mirroring
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
acceptance:
  - pathflow-config.json has task_tracker section with templates for mirroring TaskCreate/TaskUpdate events to JSONL
  - CLAUDE.md Section 4 updated with task tracker mirroring instructions
  - cf-knowledge-layer agent definition updated to handle mirrored task events
  - Tests added for the new config structure
---

# Add task tracker mirroring to pathflow-config

## Overview

Add task_tracker templates to pathflow-config.json that mirror Claude Code's internal task tracker (TaskCreate/TaskUpdate/TaskList) into the WorkGraph. This ensures the JSONL/SQLite data model is the primary source of truth, not Claude's ephemeral internal task tracker.

Work Item 1 from the plan at `.claude/plans/pathflow-task-tracker-mirroring.md`.

## Acceptance Criteria

1. pathflow-config.json has task_tracker section with templates for mirroring TaskCreate/TaskUpdate events to JSONL
2. CLAUDE.md Section 4 updated with task tracker mirroring instructions
3. cf-knowledge-layer agent definition updated to handle mirrored task events
4. Tests added for the new config structure

## Related

- PR #18 (fix/session-id-cross-teammate) was prerequisite -- now merged
