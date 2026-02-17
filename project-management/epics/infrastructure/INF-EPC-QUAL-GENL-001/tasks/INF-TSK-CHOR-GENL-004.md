---
id: INF-TSK-CHOR-GENL-004
epic_id: INF-EPC-QUAL-GENL-001
title: Audit and fill test coverage gaps
description: Ensure every script in .codeflow/scripts/ has a corresponding test file registered in test-config.json
status: todo
area_type: INF
work_type: CHOR
domain: GENL
origin: planned
file_scope: [".codeflow/scripts/", ".codeflow/config/test-config.json", ".codeflow/testing/scripts/"]
scope_policy: hard
scope_root: null
estimate: M
priority: medium
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance:
  - "Every script in .codeflow/scripts/ has a corresponding test file"
  - "All test files registered in test-config.json"
  - "./codeflow test --mode full passes clean"
  - "Coverage report shows no untested scripts"
  - "Intentional exceptions documented in test-config.json with reason"
tests: [".codeflow/testing/scripts/test-coverage.sh"]
branch: chore/test-coverage-gaps
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-CHOR-GENL-004: Audit and fill test coverage gaps

## Description

Audit all scripts in `.codeflow/scripts/` to ensure each has a corresponding test file registered in `.codeflow/config/test-config.json`. Fill coverage gaps where tests are missing. Verify the coverage report (`./codeflow test --coverage`) reflects complete coverage.

This task ensures that no script goes untested, preventing silent regressions like the pre-existing failures fixed in INF-TSK-FIX-GENL-007.

## Approach

### Phase 1: Audit

1. List all scripts in `.codeflow/scripts/` (recursively, both `.sh` and `.py`)
2. Read `test-config.json` to get the current test file registry
3. Cross-reference: identify scripts without corresponding test files
4. Identify test files not registered in `test-config.json`
5. Produce a gap report

### Phase 2: Fill gaps

1. For each script without a test file:
   - Determine if the script is testable (some may be pure config or sourced-only)
   - If testable: create a test file with at least basic tests (function exists, loads without error, key behavior verified)
   - If not testable: document the exception in `test-config.json` with a reason
2. Register all new test files in `test-config.json`

### Phase 3: Verify

1. Run `./codeflow test --mode full` to confirm all new tests pass
2. Run `./codeflow test --coverage` to verify coverage report shows no gaps
3. Verify no regressions in existing tests

## Files

### To Modify

- `.codeflow/config/test-config.json` -- Register new test files, document exceptions
- `.codeflow/testing/scripts/` -- Existing test files may need updates

### To Create

- `.codeflow/testing/scripts/test-*.sh` or `test_*.py` -- New test files for uncovered scripts

## Dependencies

### Blocked By

- INF-TSK-FIX-GENL-007 (need a clean test baseline before auditing coverage)

### Blocks

- None

## Verification

### Automated

- [ ] `./codeflow test --mode full` passes clean
- [ ] `./codeflow test --coverage` shows no untested scripts
- [ ] All new test files registered in `test-config.json`

### Manual

- [ ] Gap audit documented (which scripts were missing tests)
- [ ] Exceptions justified (why certain scripts are intentionally untested)
- [ ] No empty or placeholder test files

## Notes

- Estimate is M (medium) because the number of gap scripts is unknown until the audit runs. If more than 10 scripts lack tests, this may need to be split into sub-tasks.
- `autorun_eligible: false` because human judgment is needed to determine what constitutes an adequate test for each script.
- Test files should follow existing naming conventions: `test-{script-name}.sh` for shell, `test_{module}.py` for Python.
