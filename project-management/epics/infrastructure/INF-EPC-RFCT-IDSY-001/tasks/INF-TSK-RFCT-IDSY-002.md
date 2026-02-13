---
id: task-01KH9V2G23QJDV9M2XWFBBXNKY
format_id: INF-TSK-RFCT-IDSY-002
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "ID Generation: Shell + Python validators and generators"
description: Update shell ulid.sh and validation.sh, Python validation.py to produce/validate ULID PKs and format IDs separately. Create new generate-format-id.sh.
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".codeflow/scripts/shell-lib/ulid.sh", ".codeflow/scripts/shell-lib/validation.sh", ".codeflow/scripts/codeflow_py_lib/validation.py", ".codeflow/scripts/db/generate-format-id.sh"]
scope_policy: strict
scope_root: null
estimate: M
priority: high
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["generate_epic_id produces epic-{ulid}", "generate_task_id produces task-{ulid}", "is_valid_epic_id validates epic-{ulid} format", "is_valid_task_id validates task-{ulid} format", "New format_id validators added for {AREA}-EPC/TSK-{TYPE}-{DOMAIN}-{NNN}", "Python PATTERNS dict split into PK and format_id patterns", "generate-format-id.sh created with sequence counter"]
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

# INF-TSK-RFCT-IDSY-002: ID Generation - Shell + Python validators

## Description

Update all ID generation and validation functions to support the dual-ID system. Shell functions currently produce wrong formats. Python patterns are labeled incorrectly.

### Changes (4 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-GEN-01 | shell-lib/ulid.sh L87-94 | generate_epic_id -> "epic-{ulid}", generate_task_id -> "task-{ulid}" |
| CF-GEN-02 | shell-lib/validation.sh L78-88 | Update regex for ULID PKs, add is_valid_epic_format_id/is_valid_task_format_id |
| CF-GEN-03 | codeflow_py_lib/validation.py L13-20 | Rename PATTERNS: epic_id -> epic_format_id, add epic_pk/task_pk patterns |
| CF-GEN-04 | NEW: db/generate-format-id.sh | Sequence counter per AREA-TYPE-DOMAIN triple |

### Current vs Target

```text
# Shell - generate_epic_id()
Current: EPC-{ulid}          → Wrong
Target:  epic-{ulid}         → Correct ULID PK

# Shell - is_valid_epic_id()
Current: validates EPC-{ulid} → Wrong
Target:  validates epic-{ulid} → Correct ULID PK

# Python - PATTERNS['epic_id']
Current: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN} → Correct pattern, wrong label
Target:  PATTERNS['epic_format_id'] for format, PATTERNS['epic_pk'] for ULID
```

## Approach

1. Update ulid.sh: change generate_epic_id and generate_task_id prefixes
2. Update validation.sh: fix ULID PK regex, add format_id validators
3. Update validation.py: rename and add patterns
4. Create generate-format-id.sh for sequence-based format ID generation

## Files

### To Modify

- `.codeflow/scripts/shell-lib/ulid.sh` - Fix generate functions
- `.codeflow/scripts/shell-lib/validation.sh` - Fix validators, add format_id validators
- `.codeflow/scripts/codeflow_py_lib/validation.py` - Rename + add patterns

### To Create

- `.codeflow/scripts/db/generate-format-id.sh` - Format ID generator with sequence counter

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-001 (schema must define format_id first)

### Blocks

- INF-TSK-RFCT-IDSY-003 (business logic uses these functions)
- INF-TSK-RFCT-IDSY-004 (skill docs reference these functions)
- INF-TSK-RFCT-IDSY-005 (tests validate these functions)

## Verification

### Automated

- [ ] generate_epic_id output matches ^epic-[0-9A-Z]{26}$
- [ ] generate_task_id output matches ^task-[0-9A-Z]{26}$
- [ ] is_valid_epic_id accepts epic-{ulid}, rejects EPC-{ulid}
- [ ] is_valid_epic_format_id accepts {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}
- [ ] Python patterns correctly named and match expected formats
- [ ] generate-format-id.sh produces sequential IDs

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Section 4.2
