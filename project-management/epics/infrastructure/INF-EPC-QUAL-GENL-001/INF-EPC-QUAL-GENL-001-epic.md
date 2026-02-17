---
id: INF-EPC-QUAL-GENL-001
title: Quality Infrastructure Hardening
summary: Harden testing and review pipeline to catch regressions earlier by fixing pre-existing test failures, filling coverage gaps, and enhancing QA and review agent definitions
status: planning
area_type: INF
work_type: CHOR
domain: GENL
priority: high
is_ongoing: false
file_scope: [".codeflow/testing/", ".codeflow/testing/test-config.json", ".codeflow/scripts/codeflow_py_lib/", ".claude/agents/cf-quality-assurance.md", ".claude/agents/cf-review.md"]
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
---

# INF-EPC-QUAL-GENL-001: Quality Infrastructure Hardening

## Summary

This epic addresses quality infrastructure gaps discovered during recent development. A pre-existing test failure went undetected, test coverage has gaps, and the QA and review agent definitions lack comprehensive verification steps. The work hardens the testing and review pipeline so future regressions are caught earlier.

The root problems are:

1. **Silent test failure:** 1 test fails in `--mode full` but passes in `--mode standard`, meaning the regression goes unnoticed until full runs happen.
2. **Coverage gaps:** Not all scripts in `.codeflow/scripts/` have corresponding test files, so untested code can break silently.
3. **QA agent defaults to standard mode:** cf-quality-assurance runs `--mode standard` by default, which misses the failure above.
4. **Review agent lacks standards integration:** cf-review does not load language-specific skill files or apply security/logic checklists systematically.

## Scope

### In Scope

- Fix 1 pre-existing test failure (shell: missing `integration_tested` pattern for memory scripts)
- Audit and fill test coverage gaps for all `.codeflow/scripts/` files
- Enhance cf-quality-assurance agent definition (default to full mode, add coverage verification)
- Enhance cf-review agent definition (file-type standards loading, security checklist, logic checklist)

### Out of Scope

- Refactoring the test framework itself (run-all-tests.sh, test runner infrastructure)
- Adding new test types (performance, integration) beyond coverage gap filling
- Modifying hook scripts or enforcement policies
- Fixing the pathflow gate hook false positive bug (noted as a known issue, separate task)

## Acceptance Criteria

- [ ] Pre-existing test failure fixed and passing in `--mode full`
- [ ] Every script in `.codeflow/scripts/` has a corresponding test file registered in `test-config.json`
- [ ] `./codeflow test --mode full` runs clean across all tasks
- [ ] cf-quality-assurance agent definition defaults to `--mode full` for WS-QA
- [ ] cf-quality-assurance agent definition includes coverage verification step
- [ ] cf-review agent definition includes file-type standards loading decision tree
- [ ] cf-review agent definition includes security review checklist for all review modes
- [ ] cf-review agent definition includes logic/correctness checklist for CODE_REVIEW mode

## Tasks

| ID | Title | Status | Assignee | Priority |
|----|-------|--------|----------|----------|
| INF-TSK-FIX-GENL-007 | Fix memory script test-coverage exception failure | todo | - | high |
| INF-TSK-CHOR-GENL-004 | Audit and fill test coverage gaps | todo | - | medium |
| INF-TSK-CHOR-GENL-005 | Enhance cf-quality-assurance agent definition | todo | - | medium |
| INF-TSK-CHOR-GENL-006 | Enhance cf-review agent definition | todo | - | medium |

### Execution Order

Task 1 (INF-TSK-FIX-GENL-007) must complete first because it unblocks reliable testing. Tasks 2-4 can proceed in parallel or sequentially after Task 1, with the exception that Task 4 has no dependency on Task 1 and can start immediately.

Each task produces a separate branch and PR.

## Dependencies

### Blocked By

- None

### Blocks

- Future work quality: once merged, all subsequent sessions benefit from stricter QA and review

## Technical Notes

### Root Cause Analysis

**Shell test failure (1):** The "Memory scripts should be excepted" assertion in `.codeflow/testing/lib/test-test-coverage.sh:129-133` fails because the `integration_tested` list in `.codeflow/testing/test-config.json` has no pattern matching `.codeflow/scripts/memory/*`. The test calls `is_excepted ".codeflow/scripts/memory/cf-memory-store.py"` and expects it to return true, but no entry covers memory scripts. The fix is to add an `integration_tested` entry for the memory scripts pattern.

### Known Issue: PathFlow Gate Hook False Positive

The pathflow gate hook (`cf-pre-tool-use-pathflow-gate.sh:124-159`) uses grep substring matching on Task tool prompt text to detect role teammates. If spawn prompts for function teammates contain role teammate names (e.g., mentioning "cf-development" in a cf-git-operations spawn prompt), it triggers a false positive block. This is noted here but is out of scope for this epic.

## Related

- INF-EPC-FIX-GENL-001 (pre-phase4 gaps - where some test failures were first observed)
- INF-EPC-FEAT-GENL-001 (ongoing infrastructure - test isolation work)
