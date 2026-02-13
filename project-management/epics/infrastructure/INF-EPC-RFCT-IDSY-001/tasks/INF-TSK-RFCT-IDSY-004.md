---
id: task-01KH9V2G2VDSSG0B2TTSEWGPHE
format_id: INF-TSK-RFCT-IDSY-004
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "Skill Documentation: Update 8 skill files for dual-ID"
description: Update task management, db operations, and memory management skill documentation to reflect dual-ID system
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".claude/skills/cf-task-management/", ".claude/skills/cf-db-operations/", ".claude/skills/cf-memory-management/"]
scope_policy: strict
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["cf-task-management SKILL.md operations updated for dual-ID", "id-convention.md rewritten for dual-ID", "task-lifecycle.md task_id refs updated", "cf-db-operations SKILL.md create operations updated", "schema-reference.md has format_id columns", "query-templates.md includes format_id in queries", "cf-memory-management SKILL.md clarifies ULID in active work", "active-work-schema.md has format_id fields"]
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

# INF-TSK-RFCT-IDSY-004: Skill Documentation - Update 8 skill files

## Description

Update skill documentation to reflect the dual-ID system. These are the files that instruct AI agents how to use IDs correctly.

### Changes (8 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-SKILL-01 | cf-task-management/SKILL.md L149-286 | Update operations for dual-ID |
| CF-SKILL-02 | cf-task-management/resources/id-convention.md | Complete rewrite for dual-ID |
| CF-SKILL-03 | cf-task-management/resources/task-lifecycle.md | Update task_id references |
| CF-SKILL-04 | cf-db-operations/SKILL.md L88-152 | Update create operations |
| CF-SKILL-05 | cf-db-operations/resources/schema-reference.md L15-68 | Add format_id columns |
| CF-SKILL-06 | cf-db-operations/resources/query-templates.md L22-82 | Add format_id to queries |
| CF-SKILL-07 | cf-memory-management/SKILL.md L110-346 | Clarify ULID in active work |
| CF-SKILL-08 | cf-memory-management/resources/active-work-schema.md L8-16 | Add format_id fields |

## Approach

1. Update id-convention.md first (foundation for all other docs)
2. Update SKILL.md files for each skill
3. Update resource files with format_id references

## Files

### To Modify

- `.claude/skills/cf-task-management/SKILL.md`
- `.claude/skills/cf-task-management/resources/id-convention.md`
- `.claude/skills/cf-task-management/resources/task-lifecycle.md`
- `.claude/skills/cf-db-operations/SKILL.md`
- `.claude/skills/cf-db-operations/resources/schema-reference.md`
- `.claude/skills/cf-db-operations/resources/query-templates.md`
- `.claude/skills/cf-memory-management/SKILL.md`
- `.claude/skills/cf-memory-management/resources/active-work-schema.md`

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-002 (needs ID generation design finalized)

### Blocks

- None (documentation is consumable immediately)

## Verification

### Manual

- [ ] All 8 files consistently describe dual-ID system
- [ ] No references to old EPC-{ulid} format remain
- [ ] Usage rules table included where appropriate

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Section 4.5
