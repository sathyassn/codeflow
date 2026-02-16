---
id: INF-TSK-CHOR-GENL-001
epic_id: INF-EPC-CHOR-GENL-001
title: "Move pathflow-events.jsonl from ledger to logs directory"
description: pathflow-events.jsonl contains machine-local session telemetry (phase transitions, sentinel creation, rework events). Move from .state/ledger/ to .state/logs/ which is already gitignored.
status: todo
area_type: INF
work_type: CHOR
domain: GENL
origin: planned
file_scope: ["cf-knowledge-layer.md", ".gitignore", ".state/logs/"]
scope_policy: soft
priority: low
branch: chore/pathflow-events-to-logs
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
acceptance:
  - cf-knowledge-layer writes pathflow events to .state/logs/pathflow-events.jsonl instead of .state/ledger/
  - .gitignore entry for pathflow-events.jsonl removed from ledger section (logs/ is already ignored)
  - Any code that reads pathflow-events.jsonl updated to new path
  - Existing tests pass
---

# Move pathflow-events.jsonl from ledger to logs directory

## Overview

pathflow-events.jsonl contains machine-local session telemetry (phase transitions, sentinel creation, rework events). It should NOT be in `.state/ledger/` (which is for git-tracked Tier 0 rebuild authority data). Move the write path from `.state/ledger/pathflow-events.jsonl` to `.state/logs/pathflow-events.jsonl`. The `.state/logs/` directory is already gitignored.

## Acceptance Criteria

1. cf-knowledge-layer writes pathflow events to `.state/logs/pathflow-events.jsonl` instead of `.state/ledger/`
2. `.gitignore` entry for `pathflow-events.jsonl` removed from ledger section (logs/ is already ignored)
3. Any code that reads `pathflow-events.jsonl` updated to new path
4. Existing tests pass

## Related

- Discovered during PR #18 review -- pathflow-events is session telemetry, not project data
