---
id: task-01KH9V2G0YJWXG4JD3NYHQF0JX
format_id: INF-TSK-FIX-GENL-005
epic_id: epic-01KH9V2FYMB8M8R3GVT935CF82
epic_format_id: INF-EPC-FEAT-GENL-001
title: "Create test-isolation.sh generalized library and delete old test-hook-isolation.sh"
description: Create .codeflow/testing/lib/test-isolation.sh as the generalized replacement for test-hook-isolation.sh with auto-detect project root, TEST_TMPDIR isolation, combined cleanup trap, and all existing functionality preserved
status: in_progress
area_type: INF
work_type: FIX
domain: GENL
origin: informal
file_scope: [".codeflow/testing/lib/test-isolation.sh", ".codeflow/testing/lib/test-hook-isolation.sh"]
scope_policy: soft
scope_root: null
estimate: S
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: ["test-isolation.sh created with auto-detect root, TEST_TMPDIR, combined cleanup", "old test-hook-isolation.sh deleted", "all downstream tests updated to source new library"]
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-10T00:00:00Z
updated_at: 2026-02-10T00:00:00Z
started_at: 2026-02-10T00:00:00Z
completed_at: null
---

# INF-TSK-FIX-GENL-005: Create test-isolation.sh generalized library

## Description

Create `.codeflow/testing/lib/test-isolation.sh` as the generalized replacement for `test-hook-isolation.sh`. The new library works for ANY test needing isolation, not just hooks.

Changes from the original:

1. Auto-detect project root by walking up from `BASH_SOURCE[1]` looking for `.codeflow/` + `.claude/` marker directories instead of hardcoded `../../../..`
2. Add `TEST_TMPDIR` isolation via `mktemp -d "${TMPDIR:-/tmp}/cf-test-tmp-XXXXXX"` for tests that write temp files
3. Combined cleanup trap to remove both `TEST_REPO_ROOT` and `TEST_TMPDIR` on EXIT
4. Keep all existing functionality: REPO_ROOT isolation, .state/ dirs, git init, config/security/state script copies

Exports after sourcing: `REPO_ROOT`, `REAL_REPO_ROOT`, `TEST_REPO_ROOT`, `TEST_TMPDIR`

After creating the file, DELETE the old `test-hook-isolation.sh`.

## Approach

Read the current `test-hook-isolation.sh` to understand the full existing implementation, then create the generalized version with the four changes listed above. Delete the old file after creation.

## Files

### To Create

- `.codeflow/testing/lib/test-isolation.sh` - Generalized test isolation library

### To Modify

- None

## Dependencies

### Blocked By

- None

### Blocks

- INF-TSK-FIX-GENL-003 (update test files to use isolated REPO_ROOT temp directories)

## Verification

### Automated

- [ ] test-isolation.sh exists and is executable
- [ ] test-hook-isolation.sh is deleted
- [ ] Sourcing test-isolation.sh exports REPO_ROOT, REAL_REPO_ROOT, TEST_REPO_ROOT, TEST_TMPDIR

### Manual

- [ ] Auto-detect project root works from any nesting depth
- [ ] Combined cleanup trap removes both TEST_REPO_ROOT and TEST_TMPDIR on EXIT

## Notes

Registered via ensure-work-registered from an informal work request (Claude Code Task #1). The work was already in progress when registration was triggered. Related to INF-TSK-FIX-GENL-004 (mkdir fix is superseded by new library).
