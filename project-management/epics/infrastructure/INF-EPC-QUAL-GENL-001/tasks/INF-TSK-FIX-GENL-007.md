---
id: INF-TSK-FIX-GENL-007
epic_id: INF-EPC-QUAL-GENL-001
title: Fix pre-existing test failures
description: Fix 5 tests that fail in --mode full (4 Python config path resolution, 1 shell stale exception list)
status: todo
area_type: INF
work_type: FIX
domain: GENL
origin: planned
file_scope: [".codeflow/testing/scripts/codeflow_py_lib/test_config.py", ".codeflow/testing/scripts/test-coverage.sh", ".codeflow/config/test-config.json", ".codeflow/scripts/codeflow_py_lib/config.py"]
scope_policy: hard
scope_root: null
estimate: S
priority: high
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance:
  - "test_loads_enforcement_policy_from_default_path passes in --mode full"
  - "test_enforcement_policy_has_protected_resources passes in --mode full"
  - "test_enforcement_policy_has_security_controls passes in --mode full"
  - "test_pathflow_config_has_phases passes in --mode full"
  - "Memory scripts should be excepted assertion passes in test-coverage.sh"
  - "No regressions: all other tests still pass"
  - "Full test suite runs clean: ./codeflow test --mode full"
tests: [".codeflow/testing/scripts/codeflow_py_lib/test_config.py", ".codeflow/testing/scripts/test-coverage.sh"]
branch: fix/pre-existing-test-failures
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-FIX-GENL-007: Fix pre-existing test failures

## Description

5 tests fail in `--mode full` but pass (or are skipped) in `--mode standard`:

### Python failures (4 tests in test_config.py)

| Test | Failure Reason |
|------|---------------|
| `test_loads_enforcement_policy_from_default_path` | `load_enforcement_policy()` returns empty dict |
| `test_enforcement_policy_has_protected_resources` | Empty dict has no `protected_resources` key |
| `test_enforcement_policy_has_security_controls` | Empty dict has no `security_controls` key |
| `test_pathflow_config_has_phases` | `load_pathflow_config()` returns empty dict |

**Root cause:** Path resolution in `codeflow_py_lib/config.py` cannot find config files at default paths when tests run from a different working directory than the project root.

### Shell failure (1 test in test-coverage.sh)

| Test | Failure Reason |
|------|---------------|
| "Memory scripts should be excepted" | Exception list in `test-config.json` is stale |

**Root cause:** Memory scripts have been renamed, added, or removed since the exception list was last updated. The list in `test-config.json` no longer matches the actual scripts in `.codeflow/scripts/`.

## Approach

### Python fix strategy

1. Read `codeflow_py_lib/config.py` to understand how `load_enforcement_policy()` and `load_pathflow_config()` resolve paths
2. Determine if the fix should be in the config module (resolve paths relative to script or repo root) or in the tests (set up correct working directory or mock paths)
3. Prefer fixing path resolution in config.py if the current implementation is fragile; prefer test-side fix if config.py works correctly in production and only fails in test isolation
4. Verify fix with `pytest test_config.py -v`

### Shell fix strategy

1. Read `test-config.json` to find the stale memory script exception list
2. List actual memory scripts in `.codeflow/scripts/` to determine correct exception list
3. Update `test-config.json` to match reality
4. Verify fix with `./codeflow test --mode full`

## Files

### To Modify

- `.codeflow/testing/scripts/codeflow_py_lib/test_config.py` -- Fix path resolution in tests or update test setup
- `.codeflow/scripts/codeflow_py_lib/config.py` -- Fix path resolution if root cause is in production code
- `.codeflow/testing/scripts/test-coverage.sh` -- Verify assertions still valid after config update
- `.codeflow/config/test-config.json` -- Update stale memory script exception list

## Dependencies

### Blocked By

- None

### Blocks

- INF-TSK-CHOR-GENL-004 (coverage audit depends on a clean test baseline)
- INF-TSK-CHOR-GENL-005 (QA agent enhancement needs tests to be reliable first)

## Verification

### Automated

- [ ] `pytest .codeflow/testing/scripts/codeflow_py_lib/test_config.py -v` -- all 4 tests pass
- [ ] `./codeflow test --mode full` -- full suite passes clean
- [ ] No regressions in existing tests

### Manual

- [ ] Root causes documented in commit messages
- [ ] Config path resolution is robust (not dependent on working directory)

## Notes

- The 4 Python tests likely share a single root cause (path resolution), so fixing one should fix all 4.
- Be careful not to break production config loading when fixing test path resolution.
- The shell test fix (stale exception list) is straightforward but may reveal additional stale entries in test-config.json.
