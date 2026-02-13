---
id: task-01KH9V2G1ARR1BSXR3CZCB272R
format_id: DOC-TSK-DOCS-GENL-001
epic_id: epic-01KH9V2FZ0H5CP2RHB769SPH6J
epic_format_id: DOC-EPC-DOCS-GENL-001
title: Update SKILL.md and docs for state/sentinel path migration
status: in_progress
area_type: DOC
work_type: DOCS
domain: GENL
origin: informal
scope_policy: soft
estimate: XS
priority: normal
---

# DOC-TSK-DOCS-GENL-001: Update SKILL.md and docs for state/sentinel path migration

## Description

Update cf-security-management SKILL.md and staging-workflow.md to reflect the new path structure from the state/sentinel path migration.

## Path Migration Rules

| Old Path | New Path |
|----------|----------|
| `/tmp/claude/managed/sentinels` | `.state/sentinels/skill` |
| `/tmp/claude/managed/state` | `.state/session` |
| `/tmp/claude/managed/protected-edits` | `/tmp/claude/managed/codeflow/protected-edits` |
| `pathflow:pf-` prefix | `pathflow-pf-` prefix |

## Files to Update

- `.claude/skills/cf-security-management/SKILL.md`
- `.claude/skills/cf-security-management/resources/staging-workflow.md`
