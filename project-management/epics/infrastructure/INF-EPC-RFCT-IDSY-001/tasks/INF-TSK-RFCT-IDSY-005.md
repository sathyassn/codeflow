---
id: task-01KH9V2G36ABN85NFAEPB4EGFX
format_id: INF-TSK-RFCT-IDSY-005
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "Tests: Update all test files for dual-ID"
description: Update test files for shell-lib, Python, coordination, and hooks to use correct ULID PK and format_id patterns
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".codeflow/testing/"]
scope_policy: strict
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["test-ulid.sh tests generate_epic_id/task_id produce {prefix}-{ulid}", "test-validation.sh tests ULID PK and format_id validators separately", "test_validation.py splits PK and format_id pattern tests", "coordination test mocks use task-{ulid} format", "hook test mocks use correct active-task.json format"]
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-12T00:00:00Z
updated_at: 2026-02-12T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-RFCT-IDSY-005: Tests - Update all test files for dual-ID

## Description

Update all test files to validate the new dual-ID system. Tests must verify both ULID PK generation/validation and format_id generation/validation.

### Changes (6 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-TEST-01 | testing/scripts/shell-lib/test-ulid.sh | Update generate_* test expectations |
| CF-TEST-02 | testing/scripts/shell-lib/test-validation.sh | Update is_valid_* tests, add format_id tests |
| CF-TEST-03 | testing/scripts/codeflow_py_lib/test_validation.py | Split PK + format_id tests |
| CF-TEST-04 | testing/scripts/coordination/test_cf_claim_*.py | Update mock task_id to task-{ulid} |
| CF-TEST-05 | testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-task-sentinel.sh | Update mock active-task.json |
| CF-TEST-06 | testing/claude-hooks/session-start/test-cf-session-start-*.sh | Update mock task_id |

## Approach

1. Update shell-lib tests first (foundation)
2. Update Python tests
3. Update coordination test mocks
4. Update hook test mocks
5. Run full test suite to verify

## Files

### To Modify

- `.codeflow/testing/scripts/shell-lib/test-ulid.sh`
- `.codeflow/testing/scripts/shell-lib/test-validation.sh`
- `.codeflow/testing/scripts/codeflow_py_lib/test_validation.py`
- `.codeflow/testing/scripts/coordination/test_cf_claim_*.py` (5 files)
- `.codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-task-sentinel.sh`
- `.codeflow/testing/claude-hooks/session-start/test-cf-session-start-*.sh`

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-002 (need to know correct ID formats)
- INF-TSK-RFCT-IDSY-003 (need business logic changes to test against)

### Blocks

- None

## Verification

### Automated

- [ ] All shell-lib tests pass (test-ulid.sh, test-validation.sh)
- [ ] All Python tests pass (test_validation.py)
- [ ] All coordination tests pass
- [ ] All hook tests pass
- [ ] Full test suite: 0 failures

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Section 4.6
