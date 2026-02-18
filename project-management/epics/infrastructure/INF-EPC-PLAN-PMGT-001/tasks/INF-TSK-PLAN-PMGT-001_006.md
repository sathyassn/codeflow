---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_006
epic_id: INF-EPC-PLAN-PMGT-001
title: "Work Item Templates"
description: "Create standardized epic and task templates at project-management/templates/ with YAML frontmatter fields aligned 1:1 with DB columns. Migrate and supersede archived templates."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: ["project-management/templates/", ".codeflow/docs/archived/skills/cf-documentation-standards/resources/templates/"]
scope_policy: soft
scope_root: null
estimate: S
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "project-management/templates/epic-template.md exists with all 15 epic frontmatter fields matching DB epics table columns"
  - "project-management/templates/task-template.md exists with all 30 task frontmatter fields matching DB tasks table columns"
  - "Template field names match DB column names exactly (no aliases, no extras)"
  - "Each template field has an inline comment documenting allowed values and defaults"
  - "Templates include body section structure (headings, placeholder text) matching cf-markdown-standards"
  - "Archived templates at .codeflow/docs/archived/skills/cf-documentation-standards/resources/templates/ are not modified (read-only reference)"
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-18T19:00:00Z
updated_at: 2026-02-18T19:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-PLAN-PMGT-001_006: Work Item Templates

## Description

Create canonical epic and task templates at `project-management/templates/` that serve as the single source of truth for work item format. These templates must have YAML frontmatter fields aligned 1:1 with DB table columns, ensuring markdown artifacts always match the database schema.

Currently, templates exist in two locations:

- `.claude/skills/cf-markdown-standards/resources/templates/` -- Active skill templates (generic, not fully DB-aligned)
- `.codeflow/docs/archived/skills/cf-documentation-standards/resources/templates/` -- Archived templates (outdated)

The new templates at `project-management/templates/` will be the authoritative reference. The cf-markdown-standards templates should be updated in a follow-up to reference these, but that is out of scope for this task.

## Approach

1. Read current cf-markdown-standards epic and task templates for body structure
2. Read DB schema for complete column list (epics: 16 columns → 15 frontmatter fields, tasks: 34 columns → 30 frontmatter fields)
3. Create `project-management/templates/` directory
4. Create `epic-template.md`:
   - YAML frontmatter with all 15 epic frontmatter fields
   - Each field has inline comment: type, allowed values, default
   - Body sections from cf-markdown-standards template
5. Create `task-template.md`:
   - YAML frontmatter with all 30 task frontmatter fields
   - Each field has inline comment: type, allowed values, default
   - Body sections from cf-markdown-standards template
6. Validate: count frontmatter fields, verify field names match DB columns

## Files

### To Create

- `project-management/templates/epic-template.md` -- Canonical epic template
- `project-management/templates/task-template.md` -- Canonical task template

## Dependencies

### Blocked By

- None (independent, but benefits from Task 001 confirming final schema)

### Blocks

- None (but all future epic/task creation should use these templates)

## Verification

### Automated

- [ ] `project-management/templates/epic-template.md` exists and has valid YAML frontmatter
- [ ] `project-management/templates/task-template.md` exists and has valid YAML frontmatter
- [ ] Epic template has exactly 15 frontmatter fields
- [ ] Task template has exactly 30 frontmatter fields
- [ ] Field names extracted from templates match column names from `PRAGMA table_info(epics)` and `PRAGMA table_info(tasks)`

### Manual

- [ ] Templates render correctly in a markdown preview
- [ ] Inline comments clearly document allowed values for enum fields

## Notes

- The 30-field count for tasks excludes DB-only columns (ULID `id`, `format_id` mapped to frontmatter `id`) and stage columns (`stage`, `stage_status`, `stage_history`). Stage columns are runtime state managed by the execution engine and are not included in planning frontmatter. If a future task adds stage columns to frontmatter, the template should be updated accordingly.
- Autorun eligible: this task has clear acceptance criteria, defined file scope, and no external dependencies, making it suitable for automated execution.
