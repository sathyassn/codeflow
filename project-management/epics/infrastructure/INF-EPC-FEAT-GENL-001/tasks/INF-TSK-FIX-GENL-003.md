---
id: task-01KH9V2G05AK18NYFBN0W1TTGQ
format_id: INF-TSK-FIX-GENL-003
epic_id: epic-01KH9V2FYMB8M8R3GVT935CF82
epic_format_id: INF-EPC-FEAT-GENL-001
title: "Update test files for isolated REPO_ROOT temp directories"
description: Update 2 test files to use isolated REPO_ROOT temp directories instead of real .state/ directory for parallel test safety
status: in_progress
area_type: INF
work_type: FIX
domain: GENL
origin: informal
file_scope: [".codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-logging.sh", ".codeflow/testing/claude-hooks/user-prompt-submit/test-cf-user-prompt-submit-logging.sh"]
scope_policy: soft
scope_root: null
estimate: XS
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-09T00:00:00Z
updated_at: 2026-02-09T00:00:00Z
started_at: 2026-02-09T00:00:00Z
completed_at: null
---

# INF-TSK-FIX-GENL-003: Update test files for isolated REPO_ROOT temp directories

## Description

Update 2 test files to use isolated REPO_ROOT temp directories instead of the real project .state/ directory. This enables parallel test safety since all 28 hook scripts respect the REPO_ROOT env var override.

## Files

### To Modify

- `.codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-logging.sh`
- `.codeflow/testing/claude-hooks/user-prompt-submit/test-cf-user-prompt-submit-logging.sh`

## Verification

### Automated

- [ ] Tests still pass after changes
- [ ] No references to real .state/ directory remain in test state setup
