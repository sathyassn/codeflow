---
id: "epic-01KJBRH49DJPD4V4EQT3ZWFJJZ"
format_id: "INF-EPC-018"
title: "Infrastructure Test Reliability Fixes"
summary: "Ad-hoc fixes to test scripts for cross-platform reliability and CI compatibility"
status: archived
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: true
file_scope:
  - ".codeflow/testing/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-25T23:35:00Z"
updated_at: "2026-02-25T23:35:00Z"
---

# INF-EPC-018: Infrastructure Test Reliability Fixes

## Summary

Ad-hoc fixes to test scripts for cross-platform reliability and CI compatibility. Tasks cover issues that arise when test scripts use platform-specific patterns (echo-pipe-grep with UTF-8, macOS-only flags) that fail on Linux CI.

## Scope

### In Scope

- Test script cross-platform reliability fixes
- CI compatibility issues (Linux vs macOS divergence)
- echo-pipe-grep patterns with UTF-8/special characters

### Out of Scope

- Test feature additions
- Production hook/script changes unrelated to test reliability

## Acceptance Criteria

- [ ] All identified cross-platform reliability issues resolved
- [ ] Tests pass on both macOS and Linux CI

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-018-001 | Fix test-commands.sh cross-platform reliability | todo | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Root cause pattern: echo-pipe-grep with UTF-8 content fails on Linux due to locale/encoding differences. Fix: use printf or heredoc instead of echo, or use file-based approaches instead of pipe patterns.

## Related

- PR #76: feat/db-cli-commands (where CI failure was observed)
