---
id: "task-01KHV29AS228PFTSVQ42YF66E3"
format_id: "INF-TSK-CHOR-GENL-008"
epic_id: "INF-EPC-CHOR-GENL-001"
epic_format_id: "INF-EPC-002"
title: "Add QA pipeline to code-touching work types and fix test coverage"
description: "Add WS-QA stage to CHOR and HTFX pipelines, update CLAUDE.md pipeline tables, add pre-commit hook, fix codeflow_py_lib coverage, add GitHub Actions CI"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "GENL"
origin: informal
file_scope:
  - "pathflow-config.json"
  - "CLAUDE.md"
  - "pre-commit hook"
  - "codeflow_py_lib tests"
  - "GitHub Actions CI"
scope_policy: soft
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []
tests: []
branch: "chore/qa-pipeline-coverage"
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-19T06:15:00Z"
updated_at: "2026-02-19T06:15:00Z"
started_at: "2026-02-19T06:15:00Z"
completed_at: "2026-02-20T00:46:07Z"
stage: null
stage_status: null
stage_history: "[]"
---

# INF-TSK-CHOR-GENL-008: Add QA pipeline to code-touching work types and fix test coverage

## Description

Add the WS-QA quality gate stage to CHOR and HTFX work type pipelines in pathflow-config.json, ensuring all code-touching work types go through automated testing before PR. Also fix test coverage gaps in codeflow_py_lib, add a pre-commit hook for test gating, and create a GitHub Actions CI workflow.

## Approach

1. Update pathflow-config.json to add WS-QA to CHOR and HTFX pipelines
2. Add tasks array to WS-QA stage with WS-QA-TSK-01 operation
3. Update CLAUDE.md pipeline tables to reflect new CHOR/HTFX pipelines
4. Add pre-commit hook for test gating
5. Fix codeflow_py_lib test coverage
6. Add GitHub Actions CI workflow

## Files

### To Modify

- `.codeflow/config/pathflow/pathflow-config.json` -- Add WS-QA to CHOR/HTFX pipelines, add tasks array to WS-QA stage
- `.claude/CLAUDE.md` -- Update pipeline tables in Section 6

### To Create

- Pre-commit hook for test gating
- GitHub Actions CI workflow
- Additional codeflow_py_lib tests

## Acceptance Criteria

1. CHOR and HTFX pipelines include WS-QA stage
2. WS-QA stage has tasks array with WS-QA-TSK-01
3. CLAUDE.md pipeline tables match pathflow-config.json
4. Pre-commit hook runs tests on code-touching commits
5. codeflow_py_lib test coverage improved
6. GitHub Actions CI workflow defined

## Dependencies

### Blocked By

- None

### Blocks

- None

## Verification

### Automated

- [ ] All existing tests pass after changes
- [ ] New tests pass

### Manual

- [ ] Pipeline tables in CLAUDE.md match pathflow-config.json

## Notes

Tier 2 decision recorded: WS-QA stage will support granular task definitions (tasks array) like phases do. First stage to get this is WS-QA with WS-QA-TSK-01.
