---
id: task-01KHBEHCD1X0WXXX4MQ3Z97FBP
format_id: INF-TSK-FIX-GENL-006
epic_id: epic-01KHBEHCCZG2VMA11Z6AMTDNNJ
epic_format_id: INF-EPC-FIX-GENL-001
title: "Fix pre-Phase 4 infrastructure gaps"
description: Close gaps in infrastructure before Phase 4, including hook fixes, security enforcement alignment, testing infrastructure updates, script cleanup, and schema restructuring
status: in_progress
area_type: INF
work_type: FIX
domain: GENL
origin: informal
file_scope: [".claude/hooks/", ".codeflow/scripts/", ".codeflow/testing/", ".codeflow/config/"]
scope_policy: soft
scope_root: null
estimate: L
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance: []
tests: []
branch: fix/pre-phase4-gaps
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-13T00:00:00Z
updated_at: 2026-02-13T00:00:00Z
started_at: 2026-02-13T00:00:00Z
completed_at: null
---

# INF-TSK-FIX-GENL-006: Fix pre-Phase 4 infrastructure gaps

## Description

Close gaps in infrastructure before Phase 4 implementation. This includes:

- Hook fixes (session-start, session-end, pre-tool-use, post-tool-use, stop, user-prompt-submit)
- Security enforcement alignment (sentinel, protection, enforcement scripts)
- Testing infrastructure updates (test isolation, parallel test support, coverage)
- Script cleanup (remove unused scripts, consolidate libraries)
- Schema restructuring (dual-ID system, format ID generation)
- Settings template updates
- Git hook alignment
- State management updates (ledger, memory, work-state)

## Branch

`fix/pre-phase4-gaps`

## Files

### Scope

- `.claude/hooks/codeflow/` - All hook scripts
- `.codeflow/scripts/` - Core infrastructure scripts
- `.codeflow/testing/` - Test infrastructure
- `.codeflow/config/` - Configuration files

## Verification

### Automated

- [ ] All tests pass with updated infrastructure
- [ ] Security enforcement scripts aligned with policy
- [ ] Hook scripts function correctly
- [ ] No broken references to removed files
