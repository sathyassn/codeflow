---
id: "INF-EPC-007"
format_id: "INF-EPC-007"
title: "QA Pipeline & Test Coverage"
summary: "Add WS-QA quality gate to code-touching work type pipelines, fix test coverage gaps, add pre-commit hook test gating, and GitHub Actions CI workflow"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "QUAL"
is_ongoing: false
file_scope:
  - ".codeflow/config/pathflow/pathflow-config.json"
  - ".claude/CLAUDE.md"
  - ".codeflow/scripts/hooks/pre-commit"
  - "codeflow_py_lib/"
  - ".github/workflows/"
  - ".codeflow/testing/"
priority: normal
pr_number: 40
external_id: null
external_url: null
created_at: "2026-02-19"
completed_at: "2026-02-20"
updated_at: "2026-02-20"
---

# INF-EPC-007: QA Pipeline & Test Coverage

## Summary

Add WS-QA quality gate to code-touching work type pipelines (CHOR, HTFX, TEST), fix test coverage gaps in codeflow_py_lib, add pre-commit hook test gating, and create GitHub Actions CI workflow.

## Scope

### In Scope

- Adding WS-QA stage to CHOR, HTFX, TEST pipelines in pathflow-config.json
- Updating CLAUDE.md pipeline tables to reflect new pipelines
- Adding pre-commit hook that runs `./codeflow test --mode essential`
- Fixing codeflow_py_lib test coverage (7 Python test files, 534 tests, 97% avg)
- Creating GitHub Actions CI workflow for push/PR test gating
- Fixing pre-existing shell test failures

### Out of Scope

- Changes to WS-QA stage implementation logic
- New PathFlow phases or stage types
- Changes to the test runner itself

## Acceptance Criteria

- [x] WS-QA stage added to CHOR, HTFX, TEST pipelines in pathflow-config.json
- [x] Pre-commit hook runs `./codeflow test --mode essential`
- [x] 7 Python test files meet 85%+ per-file coverage (534 tests, 97% avg)
- [x] GitHub Actions CI workflow runs `./codeflow test --coverage` on push/PR
- [x] All shell test failures fixed (pre-existing + new)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-007-001 | Add QA pipeline to code-touching work types and fix test coverage | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Key Findings

### 1. WS-QA Stage Task Definitions

WS-QA stage supports granular task definitions (tasks array) like phases do. First stage to get this is WS-QA with WS-QA-TSK-01. This is a Tier 2 decision.

### 2. Coverage Results

7 Python test files across codeflow_py_lib achieve 97% average coverage (534 tests), exceeding the 85% per-file threshold.

## Files Changed

| File | Change |
|------|--------|
| `.codeflow/config/pathflow/pathflow-config.json` | Added WS-QA to CHOR, HTFX, TEST pipelines; added tasks array to WS-QA stage |
| `.claude/CLAUDE.md` | Updated pipeline tables in Section 6 |
| `.codeflow/scripts/hooks/pre-commit` | New pre-commit hook for test gating |
| `codeflow_py_lib/` | Additional test files for coverage |
| `.github/workflows/` | GitHub Actions CI workflow |
| `.codeflow/testing/` | Shell test fixes |

## Test Results

- All 534 Python tests pass with 97% average coverage
- All shell tests pass (pre-existing failures fixed)
- Pre-commit hook validates correctly

## Related

- Old format ID: INF-TSK-CHOR-GENL-008 (migrated from INF-EPC-002)
