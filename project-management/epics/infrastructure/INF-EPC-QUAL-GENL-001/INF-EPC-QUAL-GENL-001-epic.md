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
file_scope: [".codeflow/testing/", ".codeflow/config/test-config.json", ".codeflow/scripts/codeflow_py_lib/", ".claude/agents/cf-quality-assurance.md", ".claude/agents/cf-review.md"]
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
---

# INF-EPC-QUAL-GENL-001: Quality Infrastructure Hardening

## Summary

This epic addresses quality infrastructure gaps discovered during recent development. Pre-existing test failures went undetected, test coverage has gaps, and the QA and review agent definitions lack comprehensive verification steps. The work hardens the testing and review pipeline so future regressions are caught earlier.

The root problems are:

1. **Silent test failures:** 5 tests fail in `--mode full` but pass in `--mode standard`, meaning regressions go unnoticed until full runs happen.
2. **Coverage gaps:** Not all scripts in `.codeflow/scripts/` have corresponding test files, so untested code can break silently.
3. **QA agent defaults to standard mode:** cf-quality-assurance runs `--mode standard` by default, which misses the failures above.
4. **Review agent lacks standards integration:** cf-review does not load language-specific skill files or apply security/logic checklists systematically.

## Scope

### In Scope

- Fix 5 pre-existing test failures (4 Python, 1 shell)
- Audit and fill test coverage gaps for all `.codeflow/scripts/` files
- Enhance cf-quality-assurance agent definition (default to full mode, add coverage verification)
- Enhance cf-review agent definition (file-type standards loading, security checklist, logic checklist)

### Out of Scope

- Refactoring the test framework itself (run-all-tests.sh, test runner infrastructure)
- Adding new test types (performance, integration) beyond coverage gap filling
- Modifying hook scripts or enforcement policies
- Fixing the pathflow gate hook false positive bug (noted as a known issue, separate task)

## Acceptance Criteria

- [ ] All 5 pre-existing test failures fixed and passing in `--mode full`
- [ ] Every script in `.codeflow/scripts/` has a corresponding test file registered in `test-config.json`
- [ ] `./codeflow test --mode full` runs clean across all tasks
- [ ] cf-quality-assurance agent definition defaults to `--mode full` for WS-QA
- [ ] cf-quality-assurance agent definition includes coverage verification step
- [ ] cf-review agent definition includes file-type standards loading decision tree
- [ ] cf-review agent definition includes security review checklist for all review modes
- [ ] cf-review agent definition includes logic/correctness checklist for CODE_REVIEW mode

## Tasks

| ID | Title | Status | Work Type | Priority | Depends On |
|----|-------|--------|-----------|----------|------------|
| INF-TSK-FIX-GENL-007 | Fix pre-existing test failures | todo | FIX | high | - |
| INF-TSK-CHOR-GENL-004 | Audit and fill test coverage gaps | todo | CHOR | medium | INF-TSK-FIX-GENL-007 |
| INF-TSK-CHOR-GENL-005 | Enhance cf-quality-assurance agent definition | todo | CHOR | medium | INF-TSK-FIX-GENL-007 |
| INF-TSK-CHOR-GENL-006 | Enhance cf-review agent definition | todo | CHOR | medium | - |

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

**Python test failures (4):** `load_enforcement_policy()` and `load_pathflow_config()` in `codeflow_py_lib/config.py` return empty dicts when config files cannot be found at default paths. In the test environment, path resolution fails because tests run from a different working directory than expected. The fix must ensure either:

- Config paths are resolved relative to the script location, or
- Tests set up the correct working directory, or
- Tests mock the config file paths

**Shell test failure (1):** The "Memory scripts should be excepted" assertion in `test-coverage.sh` fails because the memory script exception list in `test-config.json` is stale (scripts have been renamed, added, or removed since the list was last updated).

### Known Issue: PathFlow Gate Hook False Positive

The pathflow gate hook (`cf-pre-tool-use-pathflow-gate.sh:124-159`) uses grep substring matching on Task tool prompt text to detect role teammates. If spawn prompts for function teammates contain role teammate names (e.g., mentioning "cf-development" in a cf-git-operations spawn prompt), it triggers a false positive block. This is noted here but is out of scope for this epic.

## Related

- INF-EPC-FIX-GENL-001 (pre-phase4 gaps - where some test failures were first observed)
- INF-EPC-FEAT-GENL-001 (ongoing infrastructure - test isolation work)
