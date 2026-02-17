---
id: INF-TSK-FIX-GENL-007
epic_id: INF-EPC-QUAL-GENL-001
title: Fix memory script test-coverage exception failure
description: Fix 1 shell test that fails in --mode full (stale integration_tested list missing memory script pattern)
status: todo
area_type: INF
work_type: FIX
domain: GENL
origin: planned
file_scope: [".codeflow/testing/test-config.json", ".codeflow/testing/lib/test-test-coverage.sh"]
scope_policy: hard
scope_root: null
estimate: XS
priority: high
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance:
  - "'Memory scripts should be excepted' assertion passes in test-test-coverage.sh"
  - "No regressions: all other tests still pass"
  - "Full test suite runs clean: ./codeflow test --mode full"
tests: [".codeflow/testing/lib/test-test-coverage.sh"]
branch: fix/pre-existing-test-failures
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-FIX-GENL-007: Fix memory script test-coverage exception failure

## Description

1 test fails in `--mode full` but passes (or is skipped) in `--mode standard`:

### Shell failure (1 test in test-test-coverage.sh)

| Test | File | Line | Failure Reason |
|------|------|------|---------------|
| "Memory scripts should be excepted" | `.codeflow/testing/lib/test-test-coverage.sh` | 129-133 | `is_excepted` returns false for `.codeflow/scripts/memory/cf-memory-store.py` |

**Root cause:** The `integration_tested` list in `.codeflow/testing/test-config.json` has no pattern matching `.codeflow/scripts/memory/*`. The test at line 129-133 calls `is_excepted ".codeflow/scripts/memory/cf-memory-store.py"` and expects it to be excepted, but no `integration_tested` entry covers memory scripts. The `scripts-memory` test group exists (with `skip: true`) but the script itself is not listed in `integration_tested`.

## Approach

1. Read `.codeflow/testing/test-config.json` to review the `integration_tested` array
2. Add an entry for memory scripts: `{ "pattern": ".codeflow/scripts/memory/*", "tested_by": "scripts/memory/ (skipped -- awaiting Go CLI Phase 7)", "reason": "Memory scripts have dedicated test group (currently skipped)" }`
3. Verify fix with `./codeflow test --mode full`

## Files

### To Modify

- `.codeflow/testing/test-config.json` -- Add memory script pattern to `integration_tested` list

### To Read (verification only)

- `.codeflow/testing/lib/test-test-coverage.sh` -- Verify assertion at lines 129-133 passes after fix
- `.codeflow/testing/lib/test-coverage.sh` -- Understand `is_excepted()` pattern matching logic

## Dependencies

### Blocked By

- None

### Blocks

- INF-TSK-CHOR-GENL-004 (coverage audit depends on a clean test baseline)
- INF-TSK-CHOR-GENL-005 (QA agent enhancement needs tests to be reliable first)

## Verification

### Automated

- [ ] `./codeflow test --mode full` -- full suite passes clean, including "Memory scripts should be excepted"
- [ ] No regressions in existing tests

### Manual

- [ ] Root cause documented in commit message
- [ ] `integration_tested` entry is consistent with existing entries in test-config.json

## Notes

- This is an XS task: single file change (add one entry to a JSON array).
- The `scripts-memory` test group in test-config.json is marked `skip: true` with note "deleted -- awaiting Go CLI Phase 7". The `integration_tested` exception should reference this context.
- Be careful to maintain valid JSON when editing test-config.json.
