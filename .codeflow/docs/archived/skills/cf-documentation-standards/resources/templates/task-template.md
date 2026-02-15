---
id: {TASK-ID}
epic_id: {EPIC-ID}
title: {Title}
description: {One-line description}
status: todo  # todo|blocked|in_progress|complete
area_type: {FRT|BKD|INF|SHR|DOC|XCUT}
work_type: {FEAT|FIX|HTFX|RFCT|DOCS|TEST|CHOR|CICD|SPKE}
domain: {domain}
origin: planned  # planned|informal|auto
file_scope: []
scope_policy: soft  # soft|hard|permissive
scope_root: null
estimate: null  # XS|S|M|L|XL
priority: normal  # low|normal|high|critical
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []  # Required for autorun-eligible tasks
tests: []  # Recommended for autorun-eligible tasks
branch: null
pr_number: null
external_id: null
external_url: null
created_at: {ISO-8601}
updated_at: {ISO-8601}
started_at: null
completed_at: null
---

# {TASK-ID}: {Title}

## Description

{What needs to be done - clear, actionable description}

## Approach

{How to implement - technical approach and strategy}

## Files

### To Modify

- `{path/to/file1}` - {what changes}
- `{path/to/file2}` - {what changes}

### To Create

- `{path/to/new-file}` - {purpose}

## Dependencies

### Blocked By

- {None or list of blocking tasks}

### Blocks

- {None or list of dependent tasks}

## Verification

### Automated

- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Linting passes

### Manual

- [ ] {Manual verification step 1}
- [ ] {Manual verification step 2}

## Notes

{Additional context, edge cases, considerations}
