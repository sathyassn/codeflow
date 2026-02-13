---
id: task-01KH9V2G0HVRFAK559EQ050WP0
format_id: INF-TSK-FIX-GENL-004
epic_id: epic-01KH9V2FYMB8M8R3GVT935CF82
epic_format_id: INF-EPC-FEAT-GENL-001
title: "Fix missing mkdir in test-hook-isolation.sh before config copy"
description: Add mkdir -p for .codeflow parent directory before cp -R of config in test-hook-isolation.sh
status: in_progress
area_type: INF
work_type: FIX
domain: GENL
origin: informal
file_scope: [".codeflow/testing/lib/test-hook-isolation.sh"]
scope_policy: soft
scope_root: null
estimate: XS
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-09T00:00:00Z
updated_at: 2026-02-09T00:00:00Z
started_at: 2026-02-09T00:00:00Z
completed_at: null
---

# INF-TSK-FIX-GENL-004: Fix missing mkdir in test-hook-isolation.sh before config copy

## Description

The `cp -R` on line 55 copies `.codeflow/config` into `$TEST_REPO_ROOT/.codeflow/config`, but the parent directory `$TEST_REPO_ROOT/.codeflow/` is never created. The error is silently suppressed by `2>/dev/null || true`. Fix by adding `mkdir -p "$TEST_REPO_ROOT/.codeflow"` before the copy.

## Approach

Add a single `mkdir -p "$TEST_REPO_ROOT/.codeflow"` line inside the existing `if` block, before the `cp -R` command.

## Files

### To Modify

- `.codeflow/testing/lib/test-hook-isolation.sh` - Add mkdir -p before cp -R of config directory

### To Create

- None

## Dependencies

### Blocked By

- None

### Blocks

- None

## Verification

### Manual

- [ ] Config directory is correctly copied into isolated test repo after fix

## Notes

The bug was discovered because the `2>/dev/null || true` suppressor hid the copy failure. Tests relying on config files in the isolated repo would silently get no config.
