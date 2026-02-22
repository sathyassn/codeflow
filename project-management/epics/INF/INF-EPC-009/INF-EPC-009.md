---
id: "INF-EPC-009"
format_id: "INF-EPC-009"
title: "Test Enforcement Requirements and Deferred Shutdown"
summary: "Add persistent test enforcement across validation scripts and agent definitions, plus document deferred teammate shutdown protocol"
status: in_progress
area_type: "INF"
work_type: "CHOR"
domain: "QUAL"
is_ongoing: false
file_scope:
  - "project-management/templates/task-template.md"
  - ".codeflow/scripts/validation/"
  - ".claude/agents/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-22"
updated_at: "2026-02-22"
---

# INF-EPC-009: Test Enforcement Requirements and Deferred Shutdown

## Summary

Add persistent test requirement enforcement across task templates, validation scripts, and agent definitions (cf-development, cf-review, cf-quality-assurance). Also document the deferred teammate shutdown protocol in CLAUDE.md. This ensures every code/script file modification mandates corresponding test file creation or update, and clarifies that on-demand teammates remain active until PF7-END.

## Scope

### In scope

- Task template test enforcement fields and guidance
- validate-task.sh tests-field check for code-producing work types
- validate-epic.sh work_type enum validation
- cf-development.md explicit test-per-code-file rule
- cf-review.md CODE_REVIEW checklist test file verification
- cf-quality-assurance.md WS-QA gate file_scope vs test-config.json cross-reference
- Deferred shutdown protocol documentation in CLAUDE.md

### Out of scope

- Go CLI phase integration
- New test framework features
- CI/CD pipeline changes

## Acceptance criteria

- [x] Task template YAML has test enforcement fields and guidance
- [x] validate-task.sh checks tests field for code-producing work types
- [x] validate-epic.sh validates work_type enum
- [x] cf-development.md has explicit test-per-code-file rule
- [x] cf-review.md CODE_REVIEW checklist includes test file verification
- [x] cf-quality-assurance.md WS-QA gate cross-references file_scope against test-config.json
- [x] All existing validation tests still pass

## Tasks

| ID | Title | Status | Estimate | Priority |
|----|-------|--------|----------|----------|
| INF-TSK-009-001 | Add persistent test enforcement requirements | complete | M | normal |

## Dependencies

### Blocked by

- None

### Blocks

- None

## Technical notes

- validate-task.sh already validates required fields, status enum, and auto_merge constraints (INF-EPC-008). This epic extends it with tests-field enforcement.
- Branch: `chore/test-enforcement-requirements`

## Related

- INF-EPC-008: PathFlow PR Verification, Merge Protection & Validation Hardening (predecessor)
